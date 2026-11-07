//! Customer actions, keyed by the input VM identity, not by display name.
//! Keep this independent of sizing exceptions: wholly excluded VMs still need a plan.
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs,
    path::Path,
};

use anyhow::{bail, Result};
use rust_xlsxwriter::{Format, Workbook};
use serde::Serialize;

use crate::{
    disk,
    models::{
        cli::Cli,
        new_model::Mapper,
        rvtools::{Vdisk, Vinfo, Vpartition},
    },
};

const CATEGORIES: [&str; 3] = [
    "Not protected by image-level backup",
    "Fix before backup",
    "Check with customer",
];
const HEADERS: [&str; 16] = [
    "category",
    "vm_name",
    "datacenter",
    "cluster",
    "host",
    "power_state",
    "os",
    "issues",
    "recommended_approach",
    "affected_disks",
    "affected_provisioned_gb",
    "vm_total_provisioned_gb",
    "in_estimate",
    "cluster_lun_group",
    "source_file",
    "scope",
];

#[derive(Debug, Serialize)]
pub struct Action {
    category: &'static str,
    vm_name: String,
    datacenter: String,
    cluster: String,
    host: String,
    power_state: String,
    os: String,
    issues: String,
    recommended_approach: String,
    affected_disks: String,
    affected_provisioned_gb: f64,
    vm_total_provisioned_gb: f64,
    in_estimate: &'static str,
    cluster_lun_group: String,
    source_file: String,
    scope: &'static str,
}

// Never group unrelated clones on UUID alone, or guess peers from size/label.
fn peer_key(d: &Vdisk) -> Option<String> {
    let known = |s: &str| !s.trim().is_empty() && !s.trim().eq_ignore_ascii_case("none");
    if !(d.is_shared || d.is_shared_bus) || !(known(&d.disk_uuid) || known(&d.raw_lun_id)) {
        return None;
    }
    let scope = if known(&d.vi_sdk) {
        d.vi_sdk.trim().to_ascii_lowercase()
    } else {
        format!("file:{}", d.vm_key.split('#').next().unwrap_or_default())
    };
    Some(format!("{scope}:{}", d.share_key()))
}

pub fn build(
    vms: &[Vinfo],
    parts: &[Vpartition],
    disks: &[Vdisk],
    cli: &Cli,
) -> Result<Vec<Action>> {
    let site_map: Vec<Mapper> = match &cli.dc_site_map {
        Some(path) => serde_json::from_str(&fs::read_to_string(path)?)?,
        None => Vec::new(),
    };
    let hide = |s: &str| {
        if cli.anonymize && !s.is_empty() {
            crate::anon(s)
        } else {
            s.to_owned()
        }
    };
    let vm_by_key: HashMap<_, _> = vms.iter().map(|v| (v.vm_key.as_str(), v)).collect();
    let mut by_vm: HashMap<&str, Vec<&Vdisk>> = HashMap::new();
    let mut peers: BTreeMap<String, BTreeSet<&str>> = BTreeMap::new();
    for d in disks
        .iter()
        .filter(|d| vm_by_key.contains_key(d.vm_key.as_str()))
    {
        by_vm.entry(&d.vm_key).or_default().push(d);
        if let Some(key) = peer_key(d) {
            peers.entry(key).or_default().insert(&d.vm_key);
        }
    }
    let guest_sizes = disk::guest_partition_sizes(parts);
    let has_parts: HashSet<_> = parts.iter().map(|p| p.vm_key.as_str()).collect();
    let excluded = |d: &&Vdisk| {
        (d.is_physical_rdm && !cli.keep_rdm) || (d.is_independent && !cli.keep_independent)
    };
    let mut rows = Vec::new();
    for v in vms {
        let attached = by_vm.get(v.vm_key.as_str()).cloned().unwrap_or_default();
        let affected: Vec<_> = attached
            .iter()
            .copied()
            .filter(|d| d.is_non_vadp())
            .collect();
        let total: f64 = attached.iter().map(|d| d.capacity).sum();
        let mut issues = Vec::new();
        let mut advice = Vec::new();
        let mut category = 3;
        if !affected.is_empty() {
            category = 0;
            if affected.iter().any(|d| d.is_physical_rdm) {
                issues.push("Physical-mode RDMs cannot be included in VM snapshots");
                advice.push(
                    "Veeam Agent in guest or storage snapshot/array integration (validate support)",
                );
            }
            if affected.iter().any(|d| d.is_independent) {
                issues.push("Independent disks are skipped by VM snapshots");
                advice.push(
                    "Veeam Agent in guest, or change disk mode after application-owner review",
                );
            }
            if affected.iter().any(|d| d.is_shared_bus) {
                issues.push("Shared SCSI bus prevents normal VM snapshots; verify protection of the remaining VM disks too");
                advice.push(if v.os.to_ascii_lowercase().contains("windows") {
                    "Veeam Agent in guest (failover cluster job for WSFC; confirm cluster type)"
                } else {
                    "Cluster-aware guest agent or application-native backup; confirm cluster type"
                });
            }
            if affected.iter().any(|d| d.is_shared) {
                issues.push("Multi-writer disks cannot be protected by normal VM snapshots");
                advice.push("Application-native backup (e.g. RMAN for Oracle RAC if applicable), cluster-aware agent, or supported array integration");
            }
        }
        let on = v.powerstate.eq_ignore_ascii_case("poweredOn");
        let tools = v
            .tools_status
            .as_deref()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase();
        if tools == "toolsnotinstalled" {
            category = category.min(1);
            issues.push("VMware Tools is missing: Tools-based guest processing, indexing and quiescing are unavailable");
            advice.push("Install VMware Tools; verify application-aware processing and guest indexing (network-based processing may be possible)");
        } else if tools == "toolsnotrunning" && on {
            category = category.min(1);
            issues.push("VMware Tools is stopped on a running VM: Tools-based guest processing, indexing and quiescing are unavailable");
            advice.push("Start VMware Tools and verify application-aware processing, guest indexing and quiescing");
        } else if tools == "toolsold" {
            category = category.min(1);
            issues.push("VMware Tools is outdated: quiescing or guest processing may fail");
            advice.push(
                "Upgrade VMware Tools and test application-aware processing and guest indexing",
            );
        }
        if let Some(g) = guest_sizes.get(&v.vm_key) {
            if !attached.is_empty()
                && g - total
                    > (total * cli.guest_gap_threshold / 100.0).max(cli.guest_gap_min_gib * 1024.0)
            {
                category = category.min(2);
                issues.push("Possible in-guest iSCSI/NFS storage: guest file systems exceed attached virtual disks (heuristic, not proof)");
                advice.push("Confirm guest-mounted storage with the customer; protect confirmed volumes using an agent or application/storage-native method");
            }
        }
        if on && !has_parts.contains(v.vm_key.as_str()) {
            category = category.min(2);
            issues.push(
                "No guest partition data for a powered-on VM: guest storage coverage is unknown",
            );
            advice.push("Check VMware Tools and guest mounts with the customer; collect fresh partition data");
        }
        if category == 3 {
            continue;
        }
        let mut groups = BTreeSet::new();
        for d in &affected {
            if let Some(key) = peer_key(d) {
                let names: BTreeSet<_> = peers[&key]
                    .iter()
                    .filter(|k| **k != v.vm_key)
                    .map(|k| hide(&vm_by_key[k].vm_name))
                    .collect();
                if !names.is_empty() {
                    groups.insert(names.into_iter().collect::<Vec<_>>().join(", "));
                }
            }
        }
        let mapped = site_map
            .iter()
            .find(|m| m.dc_names.iter().any(|dc| v.datacenter.contains(dc)));
        let source = v
            .vm_key
            .split('#')
            .next()
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| cli.rvtools_files.get(i))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let n_excluded = attached.iter().filter(|d| excluded(d)).count();
        rows.push(Action {
            category: CATEGORIES[category],
            vm_name: hide(&v.vm_name),
            datacenter: mapped
                .map(|m| m.group_name.clone())
                .unwrap_or_else(|| hide(&v.datacenter)),
            cluster: hide(&v.cluster),
            host: hide(&v.host),
            power_state: v.powerstate.clone(),
            os: v.os.clone(),
            issues: issues.join("; "),
            recommended_approach: advice.join("; "),
            affected_disks: affected
                .iter()
                .map(|d| {
                    if cli.anonymize {
                        hide(&d.disk_label)
                    } else {
                        d.disk_label.clone()
                    }
                })
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect::<Vec<_>>()
                .join(", "),
            affected_provisioned_gb: affected.iter().map(|d| d.capacity).sum::<f64>() / 1024.0,
            vm_total_provisioned_gb: total / 1024.0,
            in_estimate: if !attached.is_empty() && n_excluded == attached.len() {
                "no"
            } else if n_excluded > 0 {
                "partial"
            } else {
                "yes"
            },
            cluster_lun_group: groups.into_iter().collect::<Vec<_>>().join("; "),
            source_file: hide(&source),
            scope: if affected.is_empty() {
                "VM-level check"
            } else if affected.len() == attached.len() {
                "All attached disks"
            } else {
                "Some attached disks"
            },
        });
    }
    rows.sort_by(|a, b| {
        let rank = |r: &Action| CATEGORIES.iter().position(|c| *c == r.category).unwrap();
        (
            rank(a),
            &a.datacenter,
            &a.cluster,
            &a.vm_name,
            &a.source_file,
        )
            .cmp(&(
                rank(b),
                &b.datacenter,
                &b.cluster,
                &b.vm_name,
                &b.source_file,
            ))
    });
    Ok(rows)
}

fn fields(row: &Action) -> Result<Vec<String>> {
    let obj = serde_json::to_value(row)?;
    Ok(HEADERS
        .iter()
        .map(|h| match &obj[*h] {
            serde_json::Value::String(s) => s.clone(),
            value => value.to_string(),
        })
        .collect())
}

// CSV is commonly opened directly in Excel: don't execute inventory as formulas.
fn csv_safe(s: String) -> String {
    if s.starts_with(['=', '+', '-', '@', '\t', '\r', '\n']) {
        format!("'{s}")
    } else {
        s
    }
}
fn md_safe(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\\', "\\\\")
        .replace('|', "\\|")
        .replace(['\r', '\n'], "<br>")
}

pub fn write(path: &Path, rows: &[Action]) -> Result<()> {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => fs::write(path, serde_json::to_string_pretty(rows)?)?,
        "csv" => {
            let mut writer = csv::Writer::from_path(path)?;
            writer.write_record(HEADERS)?;
            for row in rows {
                writer.write_record(fields(row)?.into_iter().map(csv_safe))?;
            }
            writer.flush()?;
        }
        "md" => {
            let mut text = format!(
                "| {} |\n| {} |\n",
                HEADERS.join(" | "),
                vec!["---"; HEADERS.len()].join(" | ")
            );
            for row in rows {
                text.push_str(&format!(
                    "| {} |\n",
                    fields(row)?
                        .iter()
                        .map(|s| md_safe(s))
                        .collect::<Vec<_>>()
                        .join(" | ")
                ));
            }
            fs::write(path, text)?;
        }
        "xlsx" => write_xlsx(path, rows)?,
        _ => bail!("--action-report requires a .csv, .json, .md or .xlsx extension"),
    }
    Ok(())
}

fn write_xlsx(path: &Path, rows: &[Action]) -> Result<()> {
    let mut workbook = Workbook::new();
    let bold = Format::new().set_bold();
    let wrap = Format::new().set_text_wrap();
    let number = Format::new().set_num_format("0.00");
    let summary = workbook.add_worksheet().set_name("Summary")?;
    summary.write_string_with_format(0, 0, "Category", &bold)?;
    summary.write_string_with_format(0, 1, "VM count", &bold)?;
    for (i, category) in CATEGORIES.iter().enumerate() {
        summary.write_string(i as u32 + 1, 0, *category)?;
        summary.write_number(
            i as u32 + 1,
            1,
            rows.iter().filter(|r| r.category == *category).count() as f64,
        )?;
    }
    summary.write_string(5, 0, "Capacities are attached provisioned GiB (GB columns), not used sizing. Shared disks appear on each VM; do not sum rows.")?;
    summary.write_string(6, 0, "In estimate = inclusion policy, not backupability; shared storage may be counted on another VM.")?;
    summary.write_string(
        7,
        0,
        "Only actionable VMs are listed. Recommendations need customer/application validation.",
    )?;
    summary.set_column_width(0, 55)?;
    summary.set_column_width(1, 15)?;
    for (category, sheet) in CATEGORIES.iter().zip([
        "Not image-backupable",
        "Fix before backup",
        "Check with customer",
    ]) {
        let ws = workbook.add_worksheet().set_name(sheet)?;
        for (col, header) in HEADERS.iter().enumerate() {
            ws.write_string_with_format(0, col as u16, *header, &bold)?;
        }
        let records: Vec<_> = rows.iter().filter(|r| r.category == *category).collect();
        for (i, row) in records.iter().enumerate() {
            for (j, value) in fields(row)?.iter().enumerate() {
                if j == 10 || j == 11 {
                    ws.write_number_with_format(
                        i as u32 + 1,
                        j as u16,
                        value.parse::<f64>()?,
                        &number,
                    )?;
                } else {
                    ws.write_string_with_format(i as u32 + 1, j as u16, value, &wrap)?;
                }
            }
        }
        ws.set_freeze_panes(1, 0)?;
        ws.autofilter(0, 0, records.len() as u32, HEADERS.len() as u16 - 1)?;
        ws.set_column_width(0, 36)?;
        ws.set_column_range_width(1, 6, 24)?;
        ws.set_column_range_width(7, 8, 65)?;
        ws.set_column_range_width(9, 15, 28)?;
    }
    workbook.save(path)?;
    Ok(())
}

pub fn print_summary(rows: &[Action]) {
    eprintln!("Customer actions ({} VMs):", rows.len());
    for category in CATEGORIES {
        eprintln!(
            "  {category}: {}",
            rows.iter().filter(|r| r.category == category).count()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn disk() -> Vdisk {
        Vdisk {
            vm_key: "0#2".into(),
            capacity: 1024.0,
            disk_label: "Example disk".into(),
            raw_comp_mode: String::new(),
            disk_mode: String::new(),
            sharing: String::new(),
            disk_uuid: "example-uuid".into(),
            is_physical_rdm: false,
            is_independent: false,
            is_shared: true,
            shared_bus: String::new(),
            raw_lun_id: String::new(),
            vi_sdk: "Example vCenter".into(),
            is_shared_bus: false,
        }
    }
    #[test]
    fn peers_require_explicit_sharing_and_identity_and_are_scoped() {
        let a = disk();
        let mut b = a.clone();
        b.is_shared = false;
        assert!(peer_key(&b).is_none()); // Ordinary clones with the same UUID are not peers.
        b = a.clone();
        b.vi_sdk = "Other vCenter".into();
        assert_ne!(peer_key(&a), peer_key(&b));
        b.disk_uuid.clear();
        assert!(peer_key(&b).is_none());
        b.is_shared_bus = true;
        b.raw_lun_id = "example-lun".into();
        assert!(peer_key(&b).is_some());
        b.vi_sdk.clear();
        let mut c = b.clone();
        c.vm_key = "1#2".into();
        assert_ne!(peer_key(&b), peer_key(&c));
    }
    #[test]
    fn spreadsheet_and_markdown_escaping() {
        assert_eq!(csv_safe("=SUM(A1)".into()), "'=SUM(A1)");
        assert_eq!(csv_safe("@example".into()), "'@example");
        assert_eq!(csv_safe("Normal VM".into()), "Normal VM");
        assert_eq!(md_safe("a|b\n<script>"), "a\\|b<br>&lt;script&gt;");
    }
}
