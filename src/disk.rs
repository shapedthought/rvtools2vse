//! Disk awareness: physical RDMs, vPartition capping, and exceptions reporting.

use std::collections::HashMap;

use anyhow::Result;
use serde::Serialize;

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

/// Per-VM eligible (VADP-backupable) disk capacity in MiB, when vDisk rows exist.
fn eligible_mib(disks: &[&Vdisk], keep_rdm: bool) -> f64 {
    disks
        .iter()
        .filter(|d| keep_rdm || !d.is_physical_rdm)
        .map(|d| d.capacity)
        .sum()
}

/// Apply pRDM exclusion and cap guest partition sums at eligible vDisk capacity.
///
/// When `disk_vec` is empty (no vDisk tab / no rows), behaviour matches the
/// historical min(vInfo, ΣvPartition) join exactly.
pub fn apply(
    info_vec: &[Vinfo],
    grouped: &[Vpartition],
    disk_vec: &[Vdisk],
    use_vpartition: bool,
    keep_rdm: bool,
) -> DiskAwarenessResult {
    let mut group_map = HashMap::new();
    for (i, j) in grouped.iter().enumerate() {
        group_map.insert(j.vm_name.as_str(), i);
    }

    let mut disks_by_vm: HashMap<&str, Vec<&Vdisk>> = HashMap::new();
    for d in disk_vec {
        disks_by_vm.entry(d.vm_name.as_str()).or_default().push(d);
    }

    let mut exceptions = Vec::new();
    let mut combined = Vec::new();

    for info in info_vec {
        let disks = disks_by_vm.get(info.vm_name.as_str()).cloned().unwrap_or_default();
        let has_disks = !disks.is_empty();
        let prdms: Vec<&Vdisk> = disks.iter().copied().filter(|d| d.is_physical_rdm).collect();

        for d in &prdms {
            exceptions.push(Exception {
                vm_name: info.vm_name.clone(),
                disk: d.disk_label.clone(),
                reason: format!("physical_rdm ({})", d.raw_comp_mode),
                capacity_mib: d.capacity,
                action: if keep_rdm {
                    "kept (--keep-rdm)".into()
                } else {
                    "excluded from estimate".into()
                },
            });
        }

        if has_disks && !keep_rdm && prdms.len() == disks.len() {
            exceptions.push(Exception {
                vm_name: info.vm_name.clone(),
                disk: "*".into(),
                reason: "all_disks_physical_rdm".into(),
                capacity_mib: prdms.iter().map(|d| d.capacity).sum(),
                action: "vm_excluded".into(),
            });
            continue;
        }

        let elig = if has_disks {
            Some(eligible_mib(&disks, keep_rdm))
        } else {
            None
        };

        if !use_vpartition {
            combined.push(info.clone());
            continue;
        }

        if let Some(&j_idx) = group_map.get(info.vm_name.as_str()) {
            let mut guest = grouped[j_idx].capacity;
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
                // Preserve the raw (pre-cap) guest sum for the -v column.
                vpartition_capacity: Some(raw_guest),
            });
        } else {
            combined.push(info.clone());
        }
    }

    DiskAwarenessResult {
        combined,
        exceptions,
    }
}

pub fn render_exceptions(rows: &[Exception], format: SummaryFormat) -> Result<String> {
    Ok(match format {
        SummaryFormat::Json => {
            serde_json::to_string_pretty(rows).map(|s| s + "\n")?
        }
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
        }
    }

    fn part(name: &str, cap: f64) -> Vpartition {
        Vpartition {
            vm_name: name.into(),
            capacity: cap,
        }
    }

    fn disk(name: &str, cap: f64, physical: bool, label: &str) -> Vdisk {
        Vdisk {
            vm_name: name.into(),
            capacity: cap,
            disk_label: label.into(),
            raw_comp_mode: if physical {
                "physicalMode".into()
            } else {
                String::new()
            },
            is_physical_rdm: physical,
        }
    }

    #[test]
    fn no_disks_matches_historical_min() {
        let infos = vec![info("a", 1000.0), info("b", 500.0)];
        let parts = vec![part("a", 400.0)];
        let r = apply(&infos, &parts, &[], true, false);
        assert_eq!(r.combined[0].capacity, 400.0);
        assert_eq!(r.combined[1].capacity, 500.0);
        assert!(r.exceptions.is_empty());
    }

    #[test]
    fn physical_rdm_excluded_and_partition_capped() {
        // VM has 200 VMDK + 800 pRDM; guest reports 900; vInfo 1000.
        // Eligible = 200 → guest capped to 200 → min(1000,200)=200.
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 900.0)];
        let disks = vec![
            disk("vm", 200.0, false, "Hard disk 1"),
            disk("vm", 800.0, true, "Hard disk 2"),
        ];
        let r = apply(&infos, &parts, &disks, true, false);
        assert_eq!(r.combined.len(), 1);
        assert_eq!(r.combined[0].capacity, 200.0);
        assert_eq!(r.combined[0].vpartition_capacity, Some(900.0));
        assert!(r.exceptions.iter().any(|e| e.reason.starts_with("physical_rdm")));
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason == "vpartition_capped_at_vdisk"));
    }

    #[test]
    fn keep_rdm_restores_capacity() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 900.0)];
        let disks = vec![
            disk("vm", 200.0, false, "Hard disk 1"),
            disk("vm", 800.0, true, "Hard disk 2"),
        ];
        let r = apply(&infos, &parts, &disks, true, true);
        // eligible = 1000, guest 900, min(1000,900)=900
        assert_eq!(r.combined[0].capacity, 900.0);
        assert!(r.exceptions.iter().all(|e| e.action.contains("kept")));
    }

    #[test]
    fn all_prdm_excludes_vm() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 500.0)];
        let disks = vec![disk("vm", 500.0, true, "Hard disk 1")];
        let r = apply(&infos, &parts, &disks, true, false);
        assert!(r.combined.is_empty());
        assert!(r
            .exceptions
            .iter()
            .any(|e| e.reason == "all_disks_physical_rdm"));
    }

    #[test]
    fn virtual_rdm_kept() {
        let infos = vec![info("vm", 1000.0)];
        let parts = vec![part("vm", 800.0)];
        let disks = vec![Vdisk {
            vm_name: "vm".into(),
            capacity: 800.0,
            disk_label: "Hard disk 1".into(),
            raw_comp_mode: "virtualMode".into(),
            is_physical_rdm: false,
        }];
        let r = apply(&infos, &parts, &disks, true, false);
        assert_eq!(r.combined[0].capacity, 800.0);
        assert!(r.exceptions.is_empty());
    }
}
