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
    /// Non-VADP disks left out of the estimate by policy, shared storage once.
    pub excluded: Vec<ExcludedDisk>,
    pub exceptions: Vec<Exception>,
    pub impacts: Vec<FlagImpact>,
}

#[derive(Debug, Clone)]
pub struct ExcludedDisk {
    pub vm_key: String,
    pub datacenter: String,
    pub cluster: String,
    pub capacity_mib: f64,
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
    result
        .exceptions
        .extend(duplicate_name_exceptions(info_vec));
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

/// One exception per vInfo record whose VM name appears more than once.
/// With UUIDs the records are joined separately; without them the join is
/// ambiguous and child rows attach to the first record of that name in the file.
pub fn duplicate_name_exceptions(info_vec: &[Vinfo]) -> Vec<Exception> {
    let mut by_name: BTreeMap<&str, Vec<&Vinfo>> = BTreeMap::new();
    for v in info_vec {
        by_name.entry(v.vm_name.as_str()).or_default().push(v);
    }
    let mut out = Vec::new();
    for (name, vms) in by_name {
        if vms.len() < 2 {
            continue;
        }
        for v in &vms {
            let same_uuid = !v.vm_uuid.is_empty()
                && vms
                    .iter()
                    .filter(|o| o.vm_uuid.eq_ignore_ascii_case(&v.vm_uuid))
                    .count()
                    > 1;
            let action = if v.vm_uuid.is_empty() {
                "no_uuid_joined_by_name_ambiguous"
            } else if same_uuid {
                "same_uuid_in_multiple_records_check_overlapping_exports"
            } else {
                "joined_separately_by_uuid"
            };
            out.push(Exception {
                vm_name: name.to_string(),
                disk: if v.vm_uuid.is_empty() {
                    "*".into()
                } else {
                    format!("uuid:{}", v.vm_uuid)
                },
                reason: format!("duplicate_vm_name ({} records)", vms.len()),
                capacity_mib: v.capacity,
                action: action.into(),
                disk_key: None,
            });
        }
    }
    out
}

fn shared_reason(d: &Vdisk) -> String {
    if d.is_shared || !d.is_shared_bus {
        format!("shared_disk ({})", d.sharing)
    } else {
        format!("shared_disk (shared bus {})", d.shared_bus)
    }
}

fn summary_disk_key(d: &Vdisk) -> String {
    let key = d.share_key();
    if d.dedupes() || key.starts_with("uuid:") {
        key
    } else {
        // Ordinary disks without UUIDs are not shared just because their labels match.
        format!("vm:{}:{key}", d.vm_key)
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
        group_map.insert(j.vm_key.as_str(), i);
    }

    let mut disks_by_vm: HashMap<&str, Vec<&Vdisk>> = HashMap::new();
    for d in disk_vec {
        disks_by_vm.entry(d.vm_key.as_str()).or_default().push(d);
    }

    // Claim shared disks in stable VM-name order so capacity is counted once.
    let mut vm_order: Vec<&Vinfo> = info_vec.iter().collect();
    vm_order.sort_by(|a, b| (&a.vm_name, &a.vm_key).cmp(&(&b.vm_name, &b.vm_key)));

    // Owner = (vm_key, vm_name); the key keeps same-named VMs apart.
    let mut shared_owner: HashMap<String, (String, String)> = HashMap::new();
    for info in &vm_order {
        if let Some(disks) = disks_by_vm.get(info.vm_key.as_str()) {
            for d in disks {
                if d.dedupes() && !is_excluded(d, opts) {
                    shared_owner
                        .entry(d.share_key())
                        .or_insert_with(|| (info.vm_key.clone(), info.vm_name.clone()));
                }
            }
        }
    }

    // Excluded non-VADP storage, shared disks once (UUID, then LUN per vCenter),
    // attributed to the first VM in name order.
    let mut excluded = Vec::new();
    let mut excluded_seen = HashSet::new();
    for info in &vm_order {
        for d in disks_by_vm.get(info.vm_key.as_str()).into_iter().flatten() {
            if !is_excluded(d, opts) {
                continue;
            }
            if d.dedupes() && !excluded_seen.insert(d.share_key()) {
                continue;
            }
            excluded.push(ExcludedDisk {
                vm_key: info.vm_key.clone(),
                datacenter: info.datacenter.clone(),
                cluster: info.cluster.clone(),
                capacity_mib: d.capacity,
            });
        }
    }

    // Shared-SCSI-bus storage grouped by LUN/UUID, so cluster nodes can be matched up.
    let mut bus_groups: BTreeMap<String, HashSet<&str>> = BTreeMap::new();
    for d in disk_vec.iter().filter(|d| d.is_shared_bus && d.dedupes()) {
        bus_groups
            .entry(d.share_key())
            .or_default()
            .insert(d.vm_key.as_str());
    }
    let bus_group_id: HashMap<&str, usize> = bus_groups
        .keys()
        .enumerate()
        .map(|(i, k)| (k.as_str(), i + 1))
        .collect();

    let mut exceptions = Vec::new();
    let mut combined = Vec::new();
    let mut counted_shared = HashSet::new();
    let mut non_vadp_by_vm: HashMap<&str, f64> = HashMap::new();

    for info in info_vec {
        let disks = disks_by_vm
            .get(info.vm_key.as_str())
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

            if d.is_shared_bus {
                let key = d.share_key();
                let action = match bus_group_id.get(key.as_str()) {
                    Some(id) => format!(
                        "non_vadp_needs_agent; shared_lun_group_{id} ({} node(s))",
                        bus_groups[&key].len()
                    ),
                    None => "non_vadp_needs_agent; no_uuid_or_lun_to_group".into(),
                };
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: d.disk_label.clone(),
                    reason: format!("shared_scsi_bus ({})", d.shared_bus),
                    capacity_mib: d.capacity,
                    disk_key: Some(summary_disk_key(d)),
                    action,
                });
            }

            let mut counts = !is_excluded(d, opts);

            if d.dedupes() {
                let key = d.share_key();
                let (owner_key, owner) = shared_owner
                    .get(&key)
                    .map(|(k, n)| (k.as_str(), n.as_str()))
                    .unwrap_or(("", ""));
                if counts && owner_key == info.vm_key && counted_shared.insert(key) {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: shared_reason(d),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: "counted_once_on_this_vm".into(),
                    });
                } else if counts {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: shared_reason(d),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: format!("deduped_already_counted_on_{owner}"),
                    });
                    counts = false;
                } else {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: shared_reason(d),
                        capacity_mib: d.capacity,
                        disk_key: Some(summary_disk_key(d)),
                        action: "noted_also_non_vadp".into(),
                    });
                }
            }

            if counts {
                eligible += d.capacity;
                any_contributing = true;
                if d.is_non_vadp() {
                    *non_vadp_by_vm.entry(info.vm_key.as_str()).or_default() += d.capacity;
                }
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
        let has_part = group_map.contains_key(info.vm_key.as_str());
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
                capacity_mib: grouped[group_map[info.vm_key.as_str()]].capacity,
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

        let mut guest = grouped[group_map[info.vm_key.as_str()]].capacity;
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
            vm_key: info.vm_key.clone(),
            vm_uuid: info.vm_uuid.clone(),
            non_vadp_capacity: 0.0,
        });
    }

    // Non-VADP share of each VM's estimate, never more than the estimate itself.
    for v in combined.iter_mut() {
        let non_vadp = non_vadp_by_vm
            .get(v.vm_key.as_str())
            .copied()
            .unwrap_or(0.0);
        v.non_vadp_capacity = non_vadp.min(v.capacity).max(0.0);
        if v.non_vadp_capacity > 0.0 {
            exceptions.push(Exception {
                vm_name: v.vm_name.clone(),
                disk: "*".into(),
                reason: "needs_agent_or_other_method".into(),
                capacity_mib: v.non_vadp_capacity,
                disk_key: None,
                action: "kept_in_estimate_not_image_level_backupable".into(),
            });
        }
    }

    DiskAwarenessResult {
        combined,
        excluded,
        exceptions,
        impacts: Vec::new(),
    }
}

/// Thresholds for the in-guest storage heuristic.
#[derive(Debug, Clone, Copy)]
pub struct GuestGap {
    /// Minimum gap as a percentage of the VM's provisioned vDisk capacity.
    pub percent: f64,
    /// Minimum gap in GiB.
    pub min_gib: f64,
}

/// Guest partition size per VM after removing container-runtime mount noise
/// and repeated views of the same filesystem: rows are de-duplicated by
/// (VM, Disk Key, size), or by (VM, mount, size) when the Disk Key is blank.
pub fn guest_partition_sizes(parts: &[Vpartition]) -> HashMap<String, f64> {
    let mut seen = HashSet::new();
    let mut out: HashMap<String, f64> = HashMap::new();
    for p in parts {
        if p.size <= 0.0 || crate::helpers::is_container_mount(&p.mount) {
            continue;
        }
        let size = format!("{:.0}", p.size);
        let id = if p.disk_key.is_empty() {
            (p.vm_key.clone(), format!("mount:{}", p.mount), size)
        } else {
            (p.vm_key.clone(), format!("key:{}", p.disk_key), size)
        };
        if seen.insert(id) {
            *out.entry(p.vm_key.clone()).or_default() += p.size;
        }
    }
    out
}

/// Heuristic: guest file systems noticeably larger than every attached vDisk
/// suggest in-guest iSCSI/NFS/SMB volumes that image-level backup never sees.
pub fn guest_gap_exceptions(
    vms: &[Vinfo],
    parts: &[Vpartition],
    disks: &[Vdisk],
    gap: GuestGap,
) -> Vec<Exception> {
    let guest = guest_partition_sizes(parts);
    let mut vdisk: HashMap<&str, f64> = HashMap::new();
    for d in disks {
        *vdisk.entry(d.vm_key.as_str()).or_default() += d.capacity;
    }
    let mut out = Vec::new();
    for v in vms {
        let (Some(g), Some(d)) = (guest.get(&v.vm_key), vdisk.get(v.vm_key.as_str())) else {
            continue;
        };
        let diff = g - d;
        let threshold = f64::max(d * gap.percent / 100.0, gap.min_gib * 1024.0);
        if diff > threshold {
            out.push(Exception {
                vm_name: v.vm_name.clone(),
                disk: "*".into(),
                reason: "possible_in_guest_storage (heuristic)".into(),
                capacity_mib: diff,
                disk_key: None,
                action: format!(
                    "guest file systems {g:.0} MiB vs vDisks {d:.0} MiB; check for in-guest iSCSI/NFS volumes needing an agent"
                ),
            });
        }
    }
    out
}

/// VMware Tools status exceptions (vTools sheet, else vInfo `Tools`).
pub fn tools_exceptions(vms: &[Vinfo]) -> Vec<Exception> {
    let mut out = Vec::new();
    for v in vms {
        let Some(t) = v.tools_status.as_deref() else {
            continue;
        };
        let tl = t.to_ascii_lowercase();
        let powered_on = v.powerstate.to_ascii_lowercase().contains("poweredon");
        let action = if tl.contains("toolsnotinstalled") {
            "no_application_aware_processing_or_guest_indexing; install VMware Tools"
        } else if tl.contains("toolsnotrunning") && powered_on {
            "application_aware_processing_and_guest_indexing_will_fail; start VMware Tools"
        } else if tl.contains("toolsold") {
            "outdated_tools_may_break_quiescing_or_application_aware_processing; upgrade"
        } else {
            continue;
        };
        out.push(Exception {
            vm_name: v.vm_name.clone(),
            disk: "*".into(),
            reason: format!("vmware_tools ({t})"),
            capacity_mib: v.capacity,
            disk_key: None,
            action: action.into(),
        });
    }
    out
}

/// One-line image-level vs agent/other split of the estimate.
pub fn coverage_summary(vms: &[Vinfo], excluded: &[ExcludedDisk]) -> String {
    let total = vms.iter().fold(0.0, |a, v| a + v.capacity);
    let non = vms.iter().fold(0.0, |a, v| a + v.non_vadp_capacity);
    let n = vms.iter().filter(|v| v.non_vadp_capacity > 0.0).count();
    let ex = excluded.iter().fold(0.0, |a, e| a + e.capacity_mib);
    let ex_vms = excluded
        .iter()
        .map(|e| e.vm_key.as_str())
        .collect::<HashSet<_>>()
        .len();
    let tb = 1048576.0;
    format!(
        "Coverage:\n  In estimate, image-level (VADP): {:.2} TB\n  In estimate, needs agent or other method: {:.2} TB ({n} VM(s))\n  Not in estimate, needs agent or other method: {:.2} TB provisioned ({ex_vms} VM(s))\n",
        (total - non) / tb,
        non / tb,
        ex / tb
    )
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
    } else if r.starts_with("shared_scsi_bus") {
        "shared_scsi_bus"
    } else if r.starts_with("possible_in_guest_storage") {
        "in_guest_storage"
    } else if r.starts_with("vmware_tools") {
        if r.contains("toolsnotinstalled") {
            "tools_not_installed"
        } else if r.contains("toolsnotrunning") {
            "tools_not_running"
        } else {
            "tools_old"
        }
    } else if r.starts_with("needs_agent_or_other") {
        "needs_agent"
    } else if r.starts_with("duplicate_vm_name") {
        "duplicate_vm_name"
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
        "shared_scsi_bus" => {
            if count == 1 {
                "disk on a shared SCSI bus (WSFC-style, non-VADP)"
            } else {
                "disks on a shared SCSI bus (WSFC-style, non-VADP)"
            }
        }
        "in_guest_storage" => {
            if count == 1 {
                "VM with possible in-guest storage (heuristic)"
            } else {
                "VMs with possible in-guest storage (heuristic)"
            }
        }
        "tools_not_installed" => {
            if count == 1 {
                "VM without VMware Tools installed"
            } else {
                "VMs without VMware Tools installed"
            }
        }
        "tools_not_running" => {
            if count == 1 {
                "powered-on VM with VMware Tools not running"
            } else {
                "powered-on VMs with VMware Tools not running"
            }
        }
        "tools_old" => {
            if count == 1 {
                "VM with outdated VMware Tools"
            } else {
                "VMs with outdated VMware Tools"
            }
        }
        "needs_agent" => {
            if count == 1 {
                "VM with kept capacity needing an agent or other method"
            } else {
                "VMs with kept capacity needing an agent or other method"
            }
        }
        "duplicate_vm_name" => {
            if count == 1 {
                "VM record sharing its name with another VM"
            } else {
                "VM records sharing a name with another VM"
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
        if matches!(
            bucket,
            "physical_rdm" | "independent_disk" | "shared_disk" | "shared_scsi_bus"
        ) {
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
            vm_key: name.into(),
            vm_uuid: String::new(),
            non_vadp_capacity: 0.0,
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
            vm_key: name.into(),
            capacity: cap,
            size: cap,
            disk_key: String::new(),
            mount: String::new(),
        }
    }

    fn disk_base(name: &str, cap: f64, label: &str) -> Vdisk {
        Vdisk {
            vm_key: name.into(),
            capacity: cap,
            disk_label: label.into(),
            raw_comp_mode: String::new(),
            disk_mode: String::new(),
            sharing: String::new(),
            disk_uuid: String::new(),
            is_physical_rdm: false,
            is_independent: false,
            is_shared: false,
            shared_bus: String::new(),
            raw_lun_id: String::new(),
            vi_sdk: String::new(),
            is_shared_bus: false,
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
        b.vm_key = "b".into();
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

    fn shared_bus_rdm(vm: &str, lun: &str) -> Vdisk {
        let mut d = disk_base(vm, 1024.0, "Hard disk 2");
        d.raw_comp_mode = "physicalMode".into();
        d.is_physical_rdm = true;
        d.shared_bus = "physicalSharing".into();
        d.is_shared_bus = true;
        d.raw_lun_id = lun.into();
        d.vi_sdk = "vc1".into();
        d
    }

    #[test]
    fn shared_bus_rdm_with_empty_uuid_groups_by_lun_and_counts_once() {
        let vms = [info("node1", 4096.0), info("node2", 4096.0)];
        let mut disks = vec![
            disk_base("node1", 1024.0, "Hard disk 1"),
            disk_base("node2", 1024.0, "Hard disk 1"),
            shared_bus_rdm("node1", "lun-a"),
            shared_bus_rdm("node2", "lun-a"),
        ];
        disks[0].disk_uuid = "u1".into();
        disks[1].disk_uuid = "u2".into();
        let guest = [part("node1", 9000.0), part("node2", 9000.0)];
        let r = apply(&vms, &guest, &disks, opts(true, false));
        let bus: Vec<_> = r
            .exceptions
            .iter()
            .filter(|e| e.reason.starts_with("shared_scsi_bus"))
            .collect();
        assert_eq!(bus.len(), 2);
        assert!(bus
            .iter()
            .all(|e| e.action.contains("shared_lun_group_1 (2 node(s))")));
        // Counted once across both nodes (vInfo stays the ceiling).
        let total: f64 = r.combined.iter().map(|v| v.capacity).sum();
        assert_eq!(total, 3072.0);
        let non: f64 = r.combined.iter().map(|v| v.non_vadp_capacity).sum();
        assert_eq!(non, 1024.0);
        // Different LUNs are never merged even with empty UUIDs.
        let disks = vec![
            shared_bus_rdm("node1", "lun-a"),
            shared_bus_rdm("node2", "lun-b"),
        ];
        let r = apply(&vms, &guest, &disks, opts(true, false));
        let total: f64 = r.combined.iter().map(|v| v.capacity).sum();
        assert_eq!(total, 2048.0);
    }

    #[test]
    fn cloned_vms_sharing_a_uuid_are_not_merged() {
        let vms = [info("clone1", 4096.0), info("clone2", 4096.0)];
        let mut a = disk_base("clone1", 1024.0, "Hard disk 1");
        let mut b = disk_base("clone2", 1024.0, "Hard disk 1");
        a.disk_uuid = "same".into();
        b.disk_uuid = "same".into();
        a.sharing = "sharingNone".into();
        b.sharing = "sharingNone".into();
        let guest = [part("clone1", 9000.0), part("clone2", 9000.0)];
        let r = apply(&vms, &guest, &[a, b], opts(false, false));
        let total: f64 = r.combined.iter().map(|v| v.capacity).sum();
        assert_eq!(total, 2048.0);
        assert!(r.combined.iter().all(|v| v.non_vadp_capacity == 0.0));
    }

    #[test]
    fn kept_independent_and_multiwriter_are_non_vadp() {
        let vms = [info("a", 8192.0), info("b", 8192.0)];
        let mut ind = disk_base("a", 2048.0, "Hard disk 2");
        ind.disk_mode = "independent_persistent".into();
        ind.is_independent = true;
        let mut mw_a = disk_base("a", 1024.0, "Hard disk 3");
        let mut mw_b = disk_base("b", 1024.0, "Hard disk 3");
        for d in [&mut mw_a, &mut mw_b] {
            d.disk_uuid = "mw".into();
            d.sharing = "sharingMultiWriter".into();
            d.is_shared = true;
        }
        let os = disk_base("a", 1024.0, "Hard disk 1");
        let disks = [os, ind, mw_a, mw_b];
        let r = apply(&vms, &[], &disks, opts(false, true));
        let non: f64 = r.combined.iter().map(|v| v.non_vadp_capacity).sum();
        assert_eq!(non, 3072.0, "independent + multi-writer counted once");
        assert!(coverage_summary(&r.combined, &r.excluded).contains("needs agent or other method"));
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason == "needs_agent_or_other_method" && e.vm_name == "a"));
        // Excluded by default: not in the estimate, so not in the agent group.
        let r = apply(&vms, &[], &disks, opts(false, false));
        let non: f64 = r.combined.iter().map(|v| v.non_vadp_capacity).sum();
        assert_eq!(non, 1024.0);
    }

    fn gpart(vm: &str, key: &str, mount: &str, size: f64) -> Vpartition {
        let mut p = part(vm, size / 2.0);
        p.size = size;
        p.disk_key = key.into();
        p.mount = mount.into();
        p
    }

    const GAP: GuestGap = GuestGap {
        percent: 10.0,
        min_gib: 50.0,
    };

    #[test]
    fn kubelet_partition_noise_is_not_flagged() {
        let vms = [info("node", 200_000.0)];
        let disks = [disk_base("node", 122_880.0, "Hard disk 1")];
        let mut parts = vec![gpart("node", "2000", "/", 122_000.0)];
        for i in 0..30 {
            parts.push(gpart(
                "node",
                "2000",
                &format!("/var/lib/kubelet/pods/p{i}/volumes"),
                122_000.0,
            ));
            parts.push(gpart("node", "2000", &format!("/sysroot/m{i}"), 122_000.0));
            parts.push(gpart("node", "", &format!("/run/containers/{i}"), 50_000.0));
        }
        assert!(guest_gap_exceptions(&vms, &parts, &disks, GAP).is_empty());
    }

    #[test]
    fn large_unbacked_guest_volume_is_flagged() {
        let vms = [info("db", 200_000.0)];
        let disks = [disk_base("db", 102_400.0, "Hard disk 1")];
        let parts = vec![
            gpart("db", "2000", "C:\\", 100_000.0),
            gpart("db", "", "E:\\", 512_000.0),
        ];
        let e = guest_gap_exceptions(&vms, &parts, &disks, GAP);
        assert_eq!(e.len(), 1);
        assert!(e[0].reason.contains("heuristic"));
        // Small gaps stay under the GiB floor.
        let parts = vec![gpart("db", "", "E:\\", 120_000.0)];
        assert!(guest_gap_exceptions(&vms, &parts, &disks, GAP).is_empty());
    }

    #[test]
    fn tools_exceptions_cover_each_status() {
        let mut off = info_tools("off", 1.0, "toolsNotRunning");
        off.powerstate = "poweredOff".into();
        let vms = [
            info_tools("ok", 1.0, "toolsOk"),
            info_tools("old", 1.0, "toolsOld"),
            info_tools("none", 1.0, "toolsNotInstalled"),
            info_tools("stopped", 1.0, "toolsNotRunning"),
            off,
        ];
        let e = tools_exceptions(&vms);
        let names: Vec<_> = e.iter().map(|e| e.vm_name.as_str()).collect();
        assert_eq!(names, ["old", "none", "stopped"]);
        let buckets: Vec<_> = e.iter().map(|e| reason_bucket(&e.reason)).collect();
        assert_eq!(
            buckets,
            ["tools_old", "tools_not_installed", "tools_not_running"]
        );
    }

    #[test]
    fn excluded_multiwriter_independent_counted_once_and_reported() {
        let vms = [info("rac-a", 8192.0), info("rac-b", 8192.0)];
        let mut disks = vec![
            disk_base("rac-a", 1024.0, "Hard disk 1"),
            disk_base("rac-b", 1024.0, "Hard disk 1"),
        ];
        for vm in ["rac-a", "rac-b"] {
            let mut d = disk_base(vm, 4096.0, "Hard disk 2");
            d.disk_uuid = "mw-ind".into();
            d.sharing = "sharingMultiWriter".into();
            d.is_shared = true;
            d.disk_mode = "independent_persistent".into();
            d.is_independent = true;
            disks.push(d);
        }
        // An empty-UUID physical RDM shared by LUN is also counted once.
        for vm in ["rac-a", "rac-b"] {
            let mut d = disk_base(vm, 2048.0, "Hard disk 3");
            d.raw_comp_mode = "physicalMode".into();
            d.is_physical_rdm = true;
            d.shared_bus = "physicalSharing".into();
            d.is_shared_bus = true;
            d.raw_lun_id = "lun-9".into();
            d.vi_sdk = "vc1".into();
            disks.push(d);
        }
        let guest = [part("rac-a", 900.0), part("rac-b", 900.0)];
        let r = apply(&vms, &guest, &disks, opts(false, false));
        let ex: f64 = r.excluded.iter().map(|e| e.capacity_mib).sum();
        assert_eq!(ex, 6144.0);
        assert!(r.excluded.iter().all(|e| e.vm_key == "rac-a"));
        assert!(r.combined.iter().all(|v| v.non_vadp_capacity == 0.0));
        let line = coverage_summary(&r.combined, &r.excluded);
        assert!(
            line.contains(
                "Not in estimate, needs agent or other method: 0.01 TB provisioned (1 VM(s))"
            ),
            "{line}"
        );
        // Keeping both moves the storage into the estimate; nothing is left excluded.
        let r = apply(&vms, &guest, &disks, opts(true, true));
        assert!(r.excluded.is_empty());
    }
}
