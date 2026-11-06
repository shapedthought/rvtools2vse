//! Disk awareness: non-VADP disks, shared-disk de-dupe, Tools heuristics, exceptions.

use std::collections::HashMap;

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
}

#[derive(Debug, Clone)]
pub struct DiskAwarenessResult {
    pub combined: Vec<Vinfo>,
    pub exceptions: Vec<Exception>,
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
                if d.is_shared {
                    shared_owner
                        .entry(d.share_key())
                        .or_insert_with(|| info.vm_name.clone());
                }
            }
        }
    }

    let mut exceptions = Vec::new();
    let mut combined = Vec::new();

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
                    action: if opts.keep_rdm {
                        "kept (--keep-rdm)".into()
                    } else {
                        "excluded from estimate".into()
                    },
                });
            }
            if d.is_independent {
                exceptions.push(Exception {
                    vm_name: info.vm_name.clone(),
                    disk: d.disk_label.clone(),
                    reason: format!("independent_disk ({})", d.disk_mode),
                    capacity_mib: d.capacity,
                    action: if opts.keep_independent {
                        "kept (--keep-independent)".into()
                    } else {
                        "excluded from estimate".into()
                    },
                });
            }

            let mut counts = !is_excluded(d, opts);

            if d.is_shared {
                let key = d.share_key();
                let owner = shared_owner.get(&key).map(String::as_str).unwrap_or("");
                if counts && owner == info.vm_name {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
                        action: "counted_once_on_this_vm".into(),
                    });
                } else if counts {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
                        action: format!("deduped_already_counted_on_{owner}"),
                    });
                    counts = false;
                } else {
                    exceptions.push(Exception {
                        vm_name: info.vm_name.clone(),
                        disk: d.disk_label.clone(),
                        reason: format!("shared_disk ({})", d.sharing),
                        capacity_mib: d.capacity,
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
                    capacity_mib: raw_guest - cap,
                    action: format!("capped {raw_guest:.2} -> {cap:.2} MiB"),
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

pub fn print_exceptions_stderr(rows: &[Exception]) {
    if rows.is_empty() {
        return;
    }
    eprintln!("Disk awareness exceptions ({}):", rows.len());
    for r in rows {
        eprintln!(
            "  {} | {} | {} | {:.2} MiB | {}",
            r.vm_name, r.disk, r.reason, r.capacity_mib, r.action
        );
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
        let mut d1 = disk_base("vm", 200.0, "Hard disk 1");
        let mut d2 = disk_base("vm", 800.0, "Hard disk 2");
        d2.raw_comp_mode = "physicalMode".into();
        d2.is_physical_rdm = true;
        let r = apply(&infos, &parts, &[d1, d2], opts(false, false));
        assert_eq!(r.combined[0].capacity, 200.0);
        assert!(r.exceptions.iter().any(|e| e.reason.starts_with("physical_rdm")));
    }

    #[test]
    fn independent_excluded_unless_kept() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 900.0)];
        let mut d1 = disk_base("vm", 400.0, "Hard disk 1");
        let mut d2 = disk_base("vm", 600.0, "Hard disk 2");
        d2.disk_mode = "independentPersistent".into();
        d2.is_independent = true;
        let r = apply(&infos, &parts, &[d1.clone(), d2.clone()], opts(false, false));
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
        assert!(r.exceptions.iter().any(|e| e.action == "counted_once_on_this_vm"));
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
}
