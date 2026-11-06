//! Disk awareness: non-VADP disks, shared-disk de-dupe, Tools heuristics, exceptions.

use std::collections::{BTreeMap, HashMap, HashSet};

use anyhow::Result;
use serde::Serialize;

use crate::helpers::tools_unreliable;
use crate::models::rvtools::{Vdisk, Vinfo, Vpartition};
use crate::summary::SummaryFormat;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Exception {
    pub vm_name: String,
    pub disk: String,
    pub reason: String,
    pub capacity_mib: f64,
    pub action: String,
    // Internal identity for summary de-duplication. Detailed exports keep their schema.
    #[serde(skip)]
    disk_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DiskAwarenessResult {
    pub combined: Vec<Vinfo>,
    pub exceptions: Vec<Exception>,
    pub impacts: Vec<FlagImpact>,
}

#[derive(Debug, Clone)]
pub struct FlagImpact {
    pub flag: &'static str,
    pub delta_mib: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct Options {
    pub use_vpartition: bool,
    pub keep_rdm: bool,
    pub keep_independent: bool,
}

fn is_excluded(d: &Vdisk, opts: Options) -> bool {
    (!opts.keep_rdm && d.is_physical_rdm) || (!opts.keep_independent && d.is_independent)
}

/// Apply disk awareness. Empty `disk_vec` and no Tools signals keep historical behaviour.
pub fn apply(
    info_vec: &[Vinfo],
    grouped: &[Vpartition],
    disk_vec: &[Vdisk],
    opts: Options,
) -> DiskAwarenessResult {
    let mut result = apply_inner(info_vec, grouped, disk_vec, opts);
    let current_total: f64 = result.combined.iter().map(|v| v.capacity).sum();
    // Counterfactual estimates use exactly the same sizing path (including guest,
    // vInfo, Tools and shared-disk rules), changing only one option at a time.
    for (bucket, flag, kept) in [
        (
            "independent_disk",
            "--keep-independent",
            opts.keep_independent,
        ),
        ("physical_rdm", "--keep-rdm", opts.keep_rdm),
    ] {
        if !result
            .exceptions
            .iter()
            .any(|e| reason_bucket(&e.reason) == bucket)
        {
            continue;
        }
        let mut alternate_opts = opts;
        if bucket == "independent_disk" {
            alternate_opts.keep_independent = !kept;
        } else {
            alternate_opts.keep_rdm = !kept;
        }
        let alternate = apply_inner(info_vec, grouped, disk_vec, alternate_opts);
        let alternate_total: f64 = alternate.combined.iter().map(|v| v.capacity).sum();
        result.impacts.push(FlagImpact {
            flag,
            delta_mib: if kept {
                current_total - alternate_total
            } else {
                alternate_total - current_total
            },
        });
    }
    result
}

fn summary_disk_key(d: &Vdisk) -> String {
    let key = d.share_key();
    if d.is_shared || key.starts_with("uuid:") {
        key
    } else {
        // Ordinary disks without UUIDs are not shared just because their labels match.
        format!("vm:{}:{key}", d.vm_name)
    }
}

fn apply_inner(
    info_vec: &[Vinfo],
    grouped: &[Vpartition],
    disk_vec: &[Vdisk],
    opts: Options,
) -> DiskAwarenessResult {
    let mut group_map = HashMap::new();
    for (i, j) in grouped.iter().enumerate() {
        group_map.insert(j.vm_name.as_str(), i);
    }

    let mut disks_by_vm: HashMap<&str, Vec<&Vdisk>> = HashMap::new();
    for d in disk_vec {
        disks_by_vm.entry(d.vm_name.as_str()).or_default().push(d);
    }

    // Claim shared disks in stable VM-name order so capacity is counted once.
    let mut vm_order: Vec<&Vinfo> = info_vec.iter().collect();
    vm_order.sort_by(|a, b| a.vm_name.cmp(&b.vm_name));

    let mut shared_owner: HashMap<String, String> = HashMap::new();
    for info in &vm_order {
        if let Some(disks) = disks_by_vm.get(info.vm_name.as_str()) {
            for d in disks {
                if d.is_shared && !is_excluded(d, opts) {
                    shared_owner
                        .entry(d.share_key())
                        .or_insert_with(|| info.vm_name.clone());
                }
            }
        }
    }

    let mut exceptions = Vec::new();
    let mut combined = Vec::new();
    let mut counted_shared = HashSet::new();

    for info in info_vec {
        let disks = disks_by_vm
            .get(info.vm_name.as_str())
            .cloned()
            .unwrap_or_default();
        let has_disks = !disks.is_empty();

        // --- per-disk exceptions & eligibility ---
        let mut eligible = 0.0_f64;
        let mut any_contributing = false;

        for d in &disks {
            if d.is_physical_rdm {
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: d.disk_label.clone(),
                    reason: format!("physical_rdm ({})", d.raw_comp_mode),
                    capacity_mib: d.capacity,
                    disk_key: Some(summary_disk_key(d)),
                    action: if !opts.keep_rdm {
                        "excluded from estimate".into()
                    } else if is_excluded(d, opts) {
                        "excluded (also independent; --keep-independent required)".into()
                    } else {
                        "kept (--keep-rdm)".into()
                    },
                });
            }
            if d.is_independent {
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: d.disk_label.clone(),
                    reason: format!("independent_disk ({})", d.disk_mode),
                    capacity_mib: d.capacity,
                    disk_key: Some(summary_disk_key(d)),
                    action: if !opts.keep_independent {
                        "excluded from estimate".into()
                    } else if is_excluded(d, opts) {
                        "excluded (also physical RDM; --keep-rdm required)".into()
                    } else {
                        "kept (--keep-independent)".into()
                    },
                });
            }

            let mut counts = !is_excluded(d, opts);

            if d.is_shared {
                let key = d.share_key();
                let owner = shared_owner.get(&key).map(String::as_str).unwrap_or("");
                if counts && owner == info.vm_name && counted_shared.insert(key) {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: "counted_once_on_this_vm".into(),
                    });
                } else if counts {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: format!("deduped_already_counted_on_{owner}"),
                    });
                    counts = false;
                } else {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: "noted_also_non_vadp".into(),
                    });
                }
            }

            if counts {
                eligible += d.capacity;
                any_contributing = true;
            }
        }

        if has_disks && !any_contributing {
            let all_non_vadp = disks.iter().all(|d| is_excluded(d, opts));
            if all_non_vadp {
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: "*".into(),
                    reason: "no_vadp_eligible_disks".into(),
                    capacity_mib: disks.iter().map(|d| d.capacity).sum(),
                    disk_key: None,
                    action: "vm_excluded".into(),
                });
                continue;
            }
            // Shared disks already counted on another VM — keep the VM at 0 capacity.
            exceptions.push(Exception {
                vm_name: info.vm_name.clone(),
                disk: "*".into(),
                reason: "no_vadp_eligible_disks".into(),
                capacity_mib: 0.0,
                disk_key: None,
                action: "capacity_zero_after_shared_dedupe".into(),
            });
            let mut zero = info.clone();
            zero.capacity = 0.0;
            zero.vpartition_capacity = None;
            combined.push(zero);
            continue;
        }

        let elig = if has_disks { Some(eligible) } else { None };
        let has_part = group_map.contains_key(info.vm_name.as_str());
        let tools_bad = tools_unreliable(info.tools_status.as_deref());
        let powered_on = info.powerstate.to_ascii_lowercase().contains("poweredon");

        // Tools / missing-guest heuristics (exceptions only unless tools_bad forces fallback).
        if powered_on && opts.use_vpartition && !has_part {
            exceptions.push(Exception {
                vm_name: info.vm_name.clone(),
                disk: "*".into(),
                reason: match info.tools_status.as_deref() {
                    Some(t) if tools_bad => format!("no_vpartition ({t})"),
                    Some(t) => format!("no_vpartition (tools={t})"),
                    None => "no_vpartition_powered_on".into(),
                },
                capacity_mib: info.capacity,
                disk_key: None,
                action: "using_vinfo_vdisk_fallback".into(),
            });
        }
        if tools_bad && has_part && opts.use_vpartition {
            exceptions.push(Exception {
                vm_name: info.vm_name.clone(),
                disk: "*".into(),
                reason: format!(
                    "tools_unreliable ({})",
                    info.tools_status.as_deref().unwrap_or("")
                ),
                capacity_mib: grouped[group_map[info.vm_name.as_str()]].capacity,
                disk_key: None,
                action: "ignored_vpartition_using_vinfo_vdisk".into(),
            });
        }

        let use_guest = opts.use_vpartition && has_part && !tools_bad;

        if !use_guest {
            // Historical path when there is no usable guest data: vInfo capacity.
            // Do not cap at vDisk here — that would change In Use (swap/snapshots) totals.
            combined.push(info.clone());
            continue;
        }

        let mut guest = grouped[group_map[info.vm_name.as_str()]].capacity;
        let raw_guest = guest;
        if let Some(cap) = elig {
            if guest > cap {
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: "*".into(),
                    reason: "vpartition_capped_at_vdisk".into(),
                    // Only the reduction that survives the vInfo minimum affects totals.
                    capacity_mib: f64::min(info.capacity, raw_guest) - f64::min(info.capacity, cap),
                    disk_key: None,
                    action: format!(
                        "guest capped {raw_guest:.2} -> {cap:.2} MiB (before vInfo minimum)"
                    ),
                });
                guest = cap;
            }
        }
        combined.push(Vinfo {
            vm_name: info.vm_name.clone(),
            datacenter: info.datacenter.clone(),
            cluster: info.cluster.clone(),
            capacity: f64::min(info.capacity, guest),
            powerstate: info.powerstate.clone(),
            vpartition_capacity: Some(raw_guest),
            tools_status: info.tools_status.clone(),
        });
    }

    DiskAwarenessResult {
        combined,
        exceptions,
        impacts: Vec::new(),
    }
}

pub fn render_exceptions(rows: &[Exception], format: SummaryFormat) -> Result<String> {
    Ok(match format {
        SummaryFormat::Json => serde_json::to_string_pretty(rows).map(|s| s + "\n")?,
        SummaryFormat::Csv => {
            let mut wtr = csv::Writer::from_writer(vec![]);
            wtr.write_record(["vm_name", "disk", "reason", "capacity_mib", "action"])?;
            for r in rows {
                wtr.write_record([
                    r.vm_name.as_str(),
                    r.disk.as_str(),
                    r.reason.as_str(),
                    &format!("{:.2}", r.capacity_mib),
                    r.action.as_str(),
                ])?;
            }
            String::from_utf8(wtr.into_inner()?)?
        }
        SummaryFormat::Md => {
            let mut out = String::from(
                "| VM | Disk | Reason | Capacity (MiB) | Action |\n|---|---|---|---:|---|\n",
            );
            for r in rows {
                out.push_str(&format!(
                    "| {} | {} | {} | {:.2} | {} |\n",
                    r.vm_name.replace('|', "\\|"),
                    r.disk.replace('|', "\\|"),
                    r.reason,
                    r.capacity_mib,
                    r.action.replace('|', "\\|"),
                ));
            }
            out
        }
        SummaryFormat::Table => {
            use comfy_table::{
                modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS},
                presets::UTF8_FULL,
                Table,
            };
            let mut table = Table::new();
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS)
                .apply_modifier(UTF8_SOLID_INNER_BORDERS)
                .set_header(vec!["VM", "Disk", "Reason", "Capacity (MiB)", "Action"]);
            for r in rows {
                table.add_row(vec![
                    r.vm_name.clone(),
                    r.disk.clone(),
                    r.reason.clone(),
                    format!("{:.2}", r.capacity_mib),
                    r.action.clone(),
                ]);
            }
            format!("{table}\n")
        }
    })
}

/// Bucket a detailed reason string into a stable group key for summaries.
pub fn reason_bucket(reason: &str) -> &'static str {
    let r = reason.to_ascii_lowercase();
    if r.starts_with("physical_rdm") {
        "physical_rdm"
    } else if r.starts_with("independent_disk") {
        "independent_disk"
    } else if r.starts_with("shared_disk") {
        "shared_disk"
    } else if r.starts_with("vpartition_capped") {
        "vpartition_capped"
    } else if r.starts_with("tools_unreliable") {
        "tools_unreliable"
    } else if r.starts_with("no_vpartition") {
        "no_vpartition"
    } else if r.starts_with("no_vadp_eligible") || r.starts_with("all_disks_physical") {
        "no_vadp_eligible"
    } else {
        "other"
    }
}

fn bucket_label(bucket: &str, count: usize) -> String {
    let noun = match bucket {
        "physical_rdm" => {
            if count == 1 {
                "physical RDM"
            } else {
                "physical RDMs"
            }
        }
        "independent_disk" => {
            if count == 1 {
                "independent disk"
            } else {
                "independent disks"
            }
        }
        "shared_disk" => {
            if count == 1 {
                "shared/multi-writer disk noted"
            } else {
                "shared/multi-writer disks noted"
            }
        }
        "vpartition_capped" => {
            if count == 1 {
                "VM with vPartition capped at vDisk"
            } else {
                "VMs with vPartition capped at vDisk"
            }
        }
        "tools_unreliable" => {
            if count == 1 {
                "VM with Tools not running/installed (guest data ignored)"
            } else {
                "VMs with Tools not running/installed (guest data ignored)"
            }
        }
        "no_vpartition" => {
            if count == 1 {
                "powered-on VM without vPartition data"
            } else {
                "powered-on VMs without vPartition data"
            }
        }
        "no_vadp_eligible" => {
            if count == 1 {
                "VM with no VADP-eligible disks"
            } else {
                "VMs with no VADP-eligible disks"
            }
        }
        _ => {
            if count == 1 {
                "other exception"
            } else {
                "other exceptions"
            }
        }
    };
    format!("{count} {noun}")
}

/// Compact summary. Disk capacities are unique provisioned sizes, not changes
/// to the estimate; VM cap capacities are actual reductions after the vInfo min.
pub fn summarize_exceptions(rows: &[Exception], impacts: &[FlagImpact]) -> String {
    #[derive(Default)]
    struct Group {
        attachments: usize,
        disks: BTreeMap<String, f64>,
        mib: f64,
    }
    let mut groups: BTreeMap<(&str, &str), Group> = BTreeMap::new();
    let mut has_disk_groups = false;
    for r in rows {
        let bucket = reason_bucket(&r.reason);
        let disposition = if matches!(bucket, "physical_rdm" | "independent_disk") {
            if r.action.starts_with("kept") {
                "kept"
            } else {
                "excluded"
            }
        } else {
            ""
        };
        let group = groups.entry((bucket, disposition)).or_default();
        group.attachments += 1;
        group.mib += r.capacity_mib;
        if matches!(bucket, "physical_rdm" | "independent_disk" | "shared_disk") {
            has_disk_groups = true;
            let key = r
                .disk_key
                .clone()
                .unwrap_or_else(|| format!("{}:{}", r.vm_name, r.disk));
            // If exports disagree, choose the largest observed provisioned size,
            // rather than making the tally depend on input ordering.
            let size = group.disks.entry(key).or_default();
            *size = size.max(r.capacity_mib);
        }
    }
    let mut out = format!("Disk awareness: {} exception(s)\n", rows.len());
    for ((bucket, disposition), group) in groups {
        if !group.disks.is_empty() {
            let mut label = bucket_label(bucket, group.disks.len());
            if !disposition.is_empty() {
                label.push_str(&format!(" {disposition}"));
            }
            let tb: f64 = group.disks.values().sum::<f64>() / 1048576.0;
            let noun = if group.attachments == 1 {
                "attachment"
            } else {
                "attachments"
            };
            out.push_str(&format!(
                "  {label} ({} {noun}; {tb:.2} TB provisioned)\n",
                group.attachments
            ));
        } else {
            out.push_str(&format!("  {}", bucket_label(bucket, group.attachments)));
            if bucket == "vpartition_capped" {
                out.push_str(&format!(
                    " ({:.2} TB estimate reduction)",
                    group.mib / 1048576.0
                ));
            }
            out.push('\n');
        }
    }
    if has_disk_groups {
        out.push_str("Provisioned sizes are deduplicated per category; categories overlap and are not estimate changes.\n");
    }
    out.push_str(&summarize_impacts(impacts));
    out.push_str(
        "Use --verbose for the full list, or --exceptions-file path.csv for a spreadsheet.\n",
    );
    out
}

fn summarize_impacts(impacts: &[FlagImpact]) -> String {
    let mut out = String::new();
    for impact in impacts {
        out.push_str(&format!(
            "  {}: {:+.2} TB estimate change, keeping vs excluding (other options unchanged; before site mapping)\n",
            impact.flag, impact.delta_mib / 1048576.0,
        ));
    }
    out
}

pub fn print_exceptions_stderr(result: &DiskAwarenessResult, verbose: bool) {
    let rows = &result.exceptions;
    if rows.is_empty() {
        return;
    }
    if verbose {
        eprintln!("Disk awareness exceptions ({}):", rows.len());
        eprintln!("  Capacity: disk rows = provisioned MiB per attachment; vPartition caps = actual estimate reduction.");
        for r in rows {
            eprintln!(
                "  {} | {} | {} | {:.2} MiB | {}",
                r.vm_name, r.disk, r.reason, r.capacity_mib, r.action
            );
        }
        eprint!("{}", summarize_impacts(&result.impacts));
    } else {
        eprint!("{}", summarize_exceptions(rows, &result.impacts));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, cap: f64) -> Vinfo {
        Vinfo {
            vm_name: name.into(),
            datacenter: "DC".into(),
            cluster: "CL".into(),
            capacity: cap,
            powerstate: "poweredOn".into(),
            vpartition_capacity: None,
            tools_status: None,
        }
    }

    fn info_tools(name: &str, cap: f64, tools: &str) -> Vinfo {
        let mut v = info(name, cap);
        v.tools_status = Some(tools.into());
        v
    }

    fn part(name: &str, cap: f64) -> Vpartition {
        Vpartition {
            vm_name: name.into(),
            capacity: cap,
        }
    }

    fn disk_base(name: &str, cap: f64, label: &str) -> Vdisk {
        Vdisk {
            vm_name: name.into(),
            capacity: cap,
            disk_label: label.into(),
            raw_comp_mode: String::new(),
            disk_mode: String::new(),
            sharing: String::new(),
            disk_uuid: String::new(),
            is_physical_rdm: false,
            is_independent: false,
            is_shared: false,
        }
    }

    fn opts(keep_rdm: bool, keep_ind: bool) -> Options {
        Options {
            use_vpartition: true,
            keep_rdm,
            keep_independent: keep_ind,
        }
    }

    #[test]
    fn no_disks_matches_historical_min() {
        let infos = vec![info("a", 1000.0), info("b", 500.0)];
        let parts = vec![part("a", 400.0)];
        let r = apply(&infos, &parts, &[], opts(false, false));
        assert_eq!(r.combined[0].capacity, 400.0);
        assert_eq!(r.combined[1].capacity, 500.0);
    }

    #[test]
    fn physical_rdm_excluded_and_partition_capped() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 900.0)];
        let d1 = disk_base("vm", 200.0, "Hard disk 1");
        let mut d2 = disk_base("vm", 800.0, "Hard disk 2");
        d2.raw_comp_mode = "physicalMode".into();
        d2.is_physical_rdm = true;
        let r = apply(&infos, &parts, &[d1, d2], opts(false, false));
        assert_eq!(r.combined[0].capacity, 200.0);
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason.starts_with("physical_rdm")));
    }

    #[test]
    fn independent_excluded_unless_kept() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 900.0)];
        let d1 = disk_base("vm", 400.0, "Hard disk 1");
        let mut d2 = disk_base("vm", 600.0, "Hard disk 2");
        d2.disk_mode = "independentPersistent".into();
        d2.is_independent = true;
        let r = apply(
            &infos,
            &parts,
            &[d1.clone(), d2.clone()],
            opts(false, false),
        );
        assert_eq!(r.combined[0].capacity, 400.0);
        let kept = apply(&infos, &parts, &[d1, d2], opts(false, true));
        assert_eq!(kept.combined[0].capacity, 900.0);
    }

    #[test]
    fn shared_counted_once() {
        let infos = vec![info("vm_a", 2000.0), info("vm_b", 2000.0)];
        let parts = vec![part("vm_a", 1000.0), part("vm_b", 1000.0)];
        let mut a = disk_base("vm_a", 1000.0, "Shared1");
        a.is_shared = true;
        a.sharing = "sharingMultiWriter".into();
        a.disk_uuid = "UUID-1".into();
        let mut b = disk_base("vm_b", 1000.0, "Shared1");
        b.is_shared = true;
        b.sharing = "sharingMultiWriter".into();
        b.disk_uuid = "UUID-1".into();
        let r = apply(&infos, &parts, &[a, b], opts(false, false));
        assert_eq!(r.combined.len(), 2);
        let total: f64 = r.combined.iter().map(|v| v.capacity).sum();
        assert_eq!(total, 1000.0);
        assert!(r.exceptions.iter().any(|e| e.action.starts_with("deduped")));
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.action == "counted_once_on_this_vm"));
    }

    #[test]
    fn duplicate_shared_rows_on_owner_are_counted_once() {
        let infos = vec![info("vm", 4000.0)];
        let parts = vec![part("vm", 3000.0)];
        let mut shared = disk_base("vm", 1000.0, "shared");
        shared.is_shared = true;
        shared.disk_uuid = "SHARED".into();
        let r = apply(
            &infos,
            &parts,
            &[shared.clone(), shared],
            opts(false, false),
        );
        assert_eq!(r.combined[0].capacity, 1000.0);
        assert_eq!(
            r.exceptions
                .iter()
                .filter(|e| e.action == "counted_once_on_this_vm")
                .count(),
            1
        );
        let s = summarize_exceptions(&r.exceptions, &r.impacts);
        assert!(
            s.contains("1 shared/multi-writer disk noted (2 attachments;"),
            "{s}"
        );
    }

    #[test]
    fn excluded_attachment_cannot_claim_shared_ownership() {
        let infos = vec![info("a", 1000.0), info("b", 1000.0)];
        let parts = vec![part("a", 250.0), part("b", 1000.0)];
        let mut a = disk_base("a", 1000.0, "shared");
        a.is_shared = true;
        a.is_independent = true;
        a.disk_uuid = "SHARED".into();
        let mut b = a.clone();
        b.vm_name = "b".into();
        b.is_independent = false;
        let r = apply(&infos, &parts, &[a, b], opts(false, false));
        assert_eq!(r.combined.len(), 1);
        assert_eq!(r.combined[0].vm_name, "b");
        assert_eq!(r.combined[0].capacity, 1000.0);
        // Keeping can change the deterministic owner; report the actual signed delta.
        assert_eq!(r.impacts[0].delta_mib, -750.0);
    }

    #[test]
    fn no_guest_and_tools_fallback_impacts_use_actual_sizing_rules() {
        let infos = vec![
            info("missing", 1000.0),
            info_tools("tools", 1000.0, "toolsNotRunning"),
        ];
        let parts = vec![part("tools", 800.0)];
        let mut disks = Vec::new();
        for name in ["missing", "tools"] {
            disks.push(disk_base(name, 200.0, "boot"));
            let mut independent = disk_base(name, 800.0, "independent");
            independent.is_independent = true;
            disks.push(independent);
        }
        let r = apply(&infos, &parts, &disks, opts(false, false));
        assert_eq!(r.combined.iter().map(|v| v.capacity).sum::<f64>(), 2000.0);
        assert_eq!(r.impacts[0].delta_mib, 0.0);
        let summary = summarize_exceptions(&r.exceptions, &r.impacts);
        assert!(
            summary.contains("2 independent disks excluded (2 attachments;"),
            "{summary}"
        );
        assert!(
            summary.contains("--keep-independent: +0.00 TB estimate change"),
            "{summary}"
        );
    }

    #[test]
    fn tools_not_running_ignores_vpartition() {
        let infos = vec![info_tools("vm", 1000.0, "toolsNotRunning")];
        let parts = vec![part("vm", 100.0)];
        let r = apply(&infos, &parts, &[], opts(false, false));
        assert_eq!(r.combined[0].capacity, 1000.0);
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason.starts_with("tools_unreliable")));
    }

    #[test]
    fn powered_on_without_partition_noted() {
        let infos = vec![info("vm", 500.0)];
        let r = apply(&infos, &[], &[], opts(false, false));
        assert_eq!(r.combined[0].capacity, 500.0);
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason.contains("no_vpartition")));
    }

    #[test]
    fn summarize_exceptions_is_compact() {
        let rows = vec![
            Exception {
                vm_name: "a".into(),
                disk: "d1".into(),
                reason: "physical_rdm (physicalMode)".into(),
                capacity_mib: 1048576.0,
                disk_key: None,
                action: "excluded".into(),
            },
            Exception {
                vm_name: "b".into(),
                disk: "d2".into(),
                reason: "physical_rdm (physicalMode)".into(),
                capacity_mib: 2097152.0,
                disk_key: None,
                action: "excluded".into(),
            },
            Exception {
                vm_name: "c".into(),
                disk: "*".into(),
                reason: "no_vpartition_powered_on".into(),
                capacity_mib: 100.0,
                disk_key: None,
                action: "fallback".into(),
            },
        ];
        let s = summarize_exceptions(&rows, &[]);
        assert!(s.contains("3 exception(s)"));
        assert!(s.contains("2 physical RDMs excluded"));
        assert!(s.contains("2 physical RDMs excluded (2 attachments; 3.00 TB provisioned)"));
        assert!(s.contains("1 powered-on VM without vPartition data"));
        assert!(s.contains("--verbose"));
        assert!(!s.contains("physicalMode")); // detail only in verbose/file
    }

    #[test]
    fn render_exceptions_csv_is_plain() {
        let rows = vec![Exception {
            vm_name: "vm1".into(),
            disk: "Hard disk 1".into(),
            reason: "physical_rdm (physicalMode)".into(),
            capacity_mib: 1024.0,
            disk_key: None,
            action: "excluded from estimate".into(),
        }];
        let csv = render_exceptions(&rows, SummaryFormat::Csv).unwrap();
        assert!(csv.starts_with("vm_name,disk,reason,capacity_mib,action"));
        assert!(csv.contains(
            "vm1,Hard disk 1,physical_rdm (physicalMode),1024.00,excluded from estimate"
        ));
        assert!(!csv.contains("╭") && !csv.contains("│") && !csv.contains("╰"));
    }
}
