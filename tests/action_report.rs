//! Every input here is invented and constructed at runtime; no customer fixtures.
use calamine::{open_workbook, Data, Reader, Xlsx};
use rust_xlsxwriter::{Workbook, Worksheet};
use serde_json::Value;
use std::{collections::HashSet, fs, path::Path, process::Command};
use tempfile::TempDir;

const CAT: [&str; 3] = [
    "Not protected by image-level backup",
    "Fix before backup",
    "Check with customer",
];
fn text_row(ws: &mut Worksheet, row: u32, values: &[&str]) {
    for (i, value) in values.iter().enumerate() {
        ws.write_string(row, i as u16, *value).unwrap();
    }
}
fn fixture(path: &Path) {
    fixture_variant(path, false);
}
fn fixture_variant(path: &Path, duplicate_name: bool) {
    let mut book = Workbook::new();
    let info = book.add_worksheet().set_name("vInfo").unwrap();
    text_row(
        info,
        0,
        &[
            "VM",
            "VM UUID",
            "Powerstate",
            "In Use MiB",
            "Datacenter",
            "Cluster",
            "Host",
            "OS according to the configuration file",
            "Tools",
        ],
    );
    for i in 0..13 {
        let name = format!(
            "example-vm-{}",
            if duplicate_name && i == 12 { 2 } else { i }
        );
        let id = format!("example-id-{i}");
        let power = if i == 6 || i == 11 {
            "poweredOff"
        } else {
            "poweredOn"
        };
        let tools = match i {
            0 | 6 => "toolsOld",
            4 => "toolsNotInstalled",
            5 | 11 => "toolsNotRunning",
            _ => "toolsOk",
        };
        text_row(
            info,
            i + 1,
            &[
                &name,
                &id,
                power,
                "0",
                "Example DC",
                "Example Cluster",
                "Example Host",
                if i < 2 { "Windows Server" } else { "Linux" },
                tools,
            ],
        );
        info.write_number(i + 1, 3, 81920.0).unwrap();
    }
    let disk = book.add_worksheet().set_name("vDisk").unwrap();
    text_row(
        disk,
        0,
        &[
            "VM",
            "VM UUID",
            "Disk",
            "Capacity MiB",
            "Raw Comp. Mode",
            "Disk Mode",
            "Sharing mode",
            "Disk UUID",
            "Shared Bus",
            "Raw LUN ID",
            "VI SDK Server",
        ],
    );
    let mut r = 1;
    for i in 0..13 {
        let name = format!(
            "example-vm-{}",
            if duplicate_name && i == 12 { 2 } else { i }
        );
        let id = format!("example-id-{i}");
        text_row(
            disk,
            r,
            &[
                &name,
                &id,
                "Hard disk 1",
                "0",
                "",
                if i == 3 {
                    "independent_persistent"
                } else {
                    "persistent"
                },
                "sharingNone",
                &format!("example-disk-{i}"),
                if i == 10 {
                    "virtualSharing"
                } else {
                    "noSharing"
                },
                "",
                "example-vcenter",
            ],
        );
        disk.write_number(r, 3, 102400.0).unwrap();
        r += 1;
        if i < 3 || i == 12 {
            text_row(
                disk,
                r,
                &[
                    &name,
                    &id,
                    "Hard disk 2",
                    "0",
                    if i < 2 { "physicalMode" } else { "" },
                    "independent_persistent",
                    if i == 2 || i == 12 {
                        "sharingMultiWriter"
                    } else {
                        "sharingNone"
                    },
                    if i < 2 { "" } else { "example-shared-uuid" },
                    if i < 2 {
                        "physicalSharing"
                    } else {
                        "noSharing"
                    },
                    if i < 2 { "example-lun" } else { "" },
                    "example-vcenter",
                ],
            );
            disk.write_number(r, 3, 204800.0).unwrap();
            r += 1;
        }
    }
    let part = book.add_worksheet().set_name("vPartition").unwrap();
    text_row(
        part,
        0,
        &[
            "VM",
            "VM UUID",
            "Disk",
            "Disk Key",
            "Consumed MiB",
            "Capacity MiB",
        ],
    );
    let mut r = 1;
    for i in 0..13 {
        if i == 7 {
            continue;
        }
        let name = format!(
            "example-vm-{}",
            if duplicate_name && i == 12 { 2 } else { i }
        );
        let id = format!("example-id-{i}");
        text_row(part, r, &[&name, &id, "/", "example-root", "0", "0"]);
        part.write_number(r, 4, if i == 9 { 200000.0 } else { 50000.0 })
            .unwrap();
        part.write_number(r, 5, if i == 8 { 300000.0 } else { 102400.0 })
            .unwrap();
        r += 1;
    }
    book.save(path).unwrap();
}
fn run(dir: &TempDir, ext: &str, flags: &[&str]) -> (std::path::PathBuf, String) {
    let input = dir.path().join("example-input.xlsx");
    if !input.exists() {
        fixture(&input);
    }
    let output = dir.path().join(format!("actions.{ext}"));
    let result = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(input)
        .arg("--no-cache")
        .arg("--action-report")
        .arg(&output)
        .args(flags)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&result.stderr).into_owned();
    assert!(result.status.success(), "{stderr}");
    assert!(stderr.contains("Customer actions ("));
    (output, stderr)
}
fn json(path: &Path) -> Vec<Value> {
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}
fn vm<'a>(rows: &'a [Value], name: &str) -> &'a Value {
    rows.iter().find(|r| r["vm_name"] == name).unwrap()
}

#[test]
fn categories_priority_all_issues_and_no_sizing_noise() {
    let d = TempDir::new().unwrap();
    let (p, stderr) = run(&d, "json", &[]);
    let rows = json(&p);
    assert_eq!(rows.len(), 10); // 6 non-image, 2 Tools, 2 checks; powered-off filtered.
    assert_eq!(
        rows.iter()
            .map(|r| r["vm_name"].as_str().unwrap())
            .collect::<HashSet<_>>()
            .len(),
        rows.len()
    );
    let ranks: Vec<_> = rows
        .iter()
        .map(|r| CAT.iter().position(|c| r["category"] == *c).unwrap())
        .collect();
    assert!(ranks.windows(2).all(|p| p[0] <= p[1]));
    let a = vm(&rows, "example-vm-0");
    assert_eq!(a["category"], CAT[0]);
    assert!(a["issues"].as_str().unwrap().contains("outdated"));
    assert!(a["issues"].as_str().unwrap().contains("Physical-mode RDM"));
    assert!(a["recommended_approach"].as_str().unwrap().contains("WSFC"));
    assert_eq!(a["scope"], "Some attached disks");
    assert_eq!(a["in_estimate"], "partial");
    assert_eq!(a["affected_provisioned_gb"], 200.0);
    assert_eq!(a["vm_total_provisioned_gb"], 300.0);
    assert_eq!(a["cluster_lun_group"], "example-vm-1");
    assert_eq!(
        vm(&rows, "example-vm-2")["cluster_lun_group"],
        "example-vm-12"
    );
    assert_eq!(vm(&rows, "example-vm-3")["in_estimate"], "no");
    assert_eq!(vm(&rows, "example-vm-3")["scope"], "All attached disks");
    assert_eq!(vm(&rows, "example-vm-10")["category"], CAT[0]);
    assert_eq!(vm(&rows, "example-vm-4")["category"], CAT[1]);
    assert_eq!(vm(&rows, "example-vm-5")["category"], CAT[1]);
    assert_eq!(vm(&rows, "example-vm-7")["category"], CAT[2]);
    assert!(vm(&rows, "example-vm-8")["issues"]
        .as_str()
        .unwrap()
        .contains("heuristic"));
    assert!(!rows.iter().any(|r| r["vm_name"] == "example-vm-9"));
    assert!(!fs::read_to_string(p).unwrap().contains("guest capped"));
    assert!(stderr.contains("Fix before backup: 2"));
    let (p, _) = run(
        &d,
        "json",
        &["--include-powered-off", "--keep-rdm", "--keep-independent"],
    );
    let rows = json(&p);
    assert_eq!(rows.len(), 11);
    assert_eq!(vm(&rows, "example-vm-6")["category"], CAT[1]);
    assert!(!rows.iter().any(|r| r["vm_name"] == "example-vm-11"));
    assert_eq!(vm(&rows, "example-vm-0")["in_estimate"], "yes");
}

#[test]
fn formats_round_trip_and_empty_categories() {
    let d = TempDir::new().unwrap();
    let (p, _) = run(&d, "csv", &["--format", "json"]);
    let mut csv = csv::Reader::from_path(p).unwrap();
    assert_eq!(csv.headers().unwrap().len(), 16);
    assert_eq!(csv.records().count(), 10);
    let (p, _) = run(&d, "md", &[]);
    let md = fs::read_to_string(p).unwrap();
    assert_eq!(md.lines().count(), 12);
    assert!(md.contains("example-vm-0"));
    let (p, _) = run(&d, "xlsx", &[]);
    let mut book: Xlsx<_> = open_workbook(p).unwrap();
    assert_eq!(
        book.sheet_names(),
        &[
            "Summary",
            "Not image-backupable",
            "Fix before backup",
            "Check with customer"
        ]
    );
    assert_eq!(
        book.worksheet_range("Not image-backupable")
            .unwrap()
            .height(),
        7
    );
    assert_eq!(
        book.worksheet_range("Fix before backup").unwrap().height(),
        3
    );
    let disks = book.worksheet_range("Not image-backupable").unwrap();
    assert!(matches!(
        disks.get_value((1, 10)).unwrap(),
        Data::Float(_) | Data::Int(_)
    ));
    let (p, _) = run(&d, "xlsx", &["--dc-exclude", "Example DC"]);
    let mut book: Xlsx<_> = open_workbook(p).unwrap();
    assert_eq!(
        book.worksheet_range("Fix before backup").unwrap().height(),
        1
    );
}

#[test]
fn filters_site_maps_anonymize_and_partition_sizing_opt_out() {
    let d = TempDir::new().unwrap();
    let map = d.path().join("map.json");
    fs::write(
        &map,
        r#"[{"group_name":"Example Site","dc_names":["Example DC"]}]"#,
    )
    .unwrap();
    let (p, _) = run(
        &d,
        "json",
        &[
            "--anonymize",
            "--vm-exclude",
            "example-vm-1",
            "--dc-site-map",
            map.to_str().unwrap(),
        ],
    );
    let rows = json(&p);
    let hash = |s: &str| fastmurmur3::hash(s.as_bytes()).to_string();
    assert_eq!(rows.len(), 9);
    let a = vm(&rows, &hash("example-vm-0"));
    assert_eq!(a["datacenter"], "Example Site");
    assert_eq!(a["cluster_lun_group"], "");
    assert_eq!(a["host"], hash("Example Host"));
    assert_eq!(a["cluster"], hash("Example Cluster"));
    assert_eq!(a["source_file"], hash("example-input.xlsx"));
    assert_eq!(
        vm(&rows, &hash("example-vm-2"))["cluster_lun_group"],
        hash("example-vm-12")
    );
    let (p, _) = run(&d, "json", &["--anonymize"]);
    let rows = json(&p);
    assert_eq!(
        vm(&rows, &hash("example-vm-0"))["datacenter"],
        hash("Example DC")
    );
    for ext in ["csv", "md", "xlsx"] {
        let (p, _) = run(&d, ext, &["--anonymize"]);
        if ext != "xlsx" {
            let s = fs::read_to_string(p).unwrap();
            assert!(!s.contains("example-vm"));
            assert!(!s.contains("Example Host"));
        } else {
            let mut book: Xlsx<_> = open_workbook(p).unwrap();
            let s = format!(
                "{:?}",
                book.worksheet_range("Not image-backupable").unwrap()
            );
            assert!(!s.contains("example-vm"));
        }
    }
    let (p, _) = run(&d, "json", &["--do-not-use-vpartition"]);
    assert_eq!(json(&p).len(), 10); // Sizing opt-out must not manufacture missing-data issues.
}

#[test]
fn unknown_extension_is_an_error() {
    let d = TempDir::new().unwrap();
    let input = d.path().join("input.xlsx");
    fixture(&input);
    let result = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(input)
        .arg("--action-report")
        .arg(d.path().join("actions.txt"))
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(String::from_utf8_lossy(&result.stderr).contains("requires a .csv"));
}

#[test]
fn action_output_does_not_change_exceptions_or_summary() {
    let d = TempDir::new().unwrap();
    let input = d.path().join("input.xlsx");
    fixture(&input);
    for flag in ["", "--do-not-use-vpartition"] {
        let go = |actions: bool| {
            let exceptions = d.path().join("exceptions.json");
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_rvsum"));
            cmd.arg("-r")
                .arg(&input)
                .arg("--no-cache")
                .arg("--exceptions-file")
                .arg(&exceptions);
            if !flag.is_empty() {
                cmd.arg(flag);
            }
            if actions {
                cmd.arg("--action-report")
                    .arg(d.path().join("actions.json"));
            }
            let out = cmd.output().unwrap();
            assert!(out.status.success());
            (out.stdout, fs::read(exceptions).unwrap())
        };
        assert_eq!(go(false), go(true));
    }
}

#[test]
fn combined_workbooks_preserve_source_identity() {
    let d = TempDir::new().unwrap();
    let a = d.path().join("a.xlsx");
    let b = d.path().join("b.xlsx");
    fixture(&a);
    fixture(&b);
    let report = d.path().join("actions.json");
    let result = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(a)
        .arg(b)
        .arg("--action-report")
        .arg(&report)
        .arg("--no-cache")
        .output()
        .unwrap();
    assert!(result.status.success());
    let rows = json(&report);
    assert_eq!(rows.len(), 20);
    assert_eq!(
        rows.iter()
            .map(|r| (
                r["source_file"].as_str().unwrap(),
                r["vm_name"].as_str().unwrap()
            ))
            .collect::<HashSet<_>>()
            .len(),
        20
    );
}

#[test]
fn same_display_name_does_not_merge_distinct_vms() {
    let d = TempDir::new().unwrap();
    fixture_variant(&d.path().join("example-input.xlsx"), true);
    let (p, _) = run(&d, "json", &[]);
    let rows = json(&p);
    assert_eq!(rows.len(), 10);
    assert_eq!(
        rows.iter()
            .filter(|r| r["vm_name"] == "example-vm-2")
            .count(),
        2
    );
}

#[test]
fn cached_action_report_preserves_os_and_host_metadata() {
    let d = TempDir::new().unwrap();
    let input = d.path().join("input.xlsx");
    fixture(&input);
    let output = d.path().join("actions.json");
    let go = || {
        let result = Command::new(env!("CARGO_BIN_EXE_rvsum"))
            .arg("-r")
            .arg(&input)
            .arg("--action-report")
            .arg(&output)
            .arg("--verbose")
            .env("XDG_CACHE_HOME", d.path().join("cache"))
            .output()
            .unwrap();
        assert!(result.status.success());
        (
            fs::read(&output).unwrap(),
            String::from_utf8_lossy(&result.stderr).into_owned(),
        )
    };
    let (cold, miss) = go();
    let (warm, hit) = go();
    assert_eq!(cold, warm);
    assert!(miss.contains("cache miss"));
    assert!(hit.contains("cache hit"));
    let rows = json(&output);
    assert_eq!(vm(&rows, "example-vm-0")["host"], "Example Host");
    assert_eq!(vm(&rows, "example-vm-0")["os"], "Windows Server");
}
