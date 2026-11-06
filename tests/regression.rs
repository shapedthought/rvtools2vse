//! Regression tests run against the built binary and the synthetic workbooks in
//! testdata/ (see testdata/generate_testdata.py).

use std::process::Command;

const BASELINE: &str = "testdata/regression_baseline.xlsx";
const NO_PART_POWER: &str = "testdata/regression_no_part_powerstate.xlsx";

fn run(args: &[&str]) -> (bool, String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rvtools2vse"))
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .args(args)
        .output()
        .expect("run binary");
    (
        out.status.success(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

fn totals(args: &[&str]) -> (f64, u64) {
    let mut a = args.to_vec();
    a.extend(["-s", "--format", "json"]);
    let (ok, stdout, stderr) = run(&a);
    assert!(ok, "failed: {stderr}");
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    (
        v["totals"]["capacity_tb"].as_f64().unwrap(),
        v["totals"]["vm_count"].as_u64().unwrap(),
    )
}

#[test]
fn baseline_totals() {
    assert_eq!(totals(&["-r", BASELINE]), (3.75, 4));
    assert_eq!(totals(&["-r", BASELINE, "-i"]), (4.25, 5));
    assert_eq!(totals(&["-r", BASELINE, "-d"]), (4.5, 4));
    assert_eq!(totals(&["-r", BASELINE, "--flatten"]), (3.75, 4));
    assert_eq!(totals(&["-r", BASELINE, "--flatten-site"]), (3.75, 4));
}

#[test]
fn baseline_csv_rows() {
    let (ok, stdout, _) = run(&["-r", BASELINE, "-s", "--format", "csv"]);
    assert!(ok);
    assert_eq!(
        stdout,
        "datacenter,cluster,capacity_tb,vm_count\nDC-A,CL2,2.00,1\nDC-A,CL1,1.00,2\nDC-B,CL3,0.75,1\n"
    );
}

#[test]
fn vpartition_without_powerstate_does_not_fail() {
    let (ok, _, stderr) = run(&["-r", NO_PART_POWER, "-s"]);
    assert!(ok, "should not fail: {stderr}");
    assert!(stderr.contains("no Powerstate column"));
    assert_eq!(totals(&["-r", NO_PART_POWER]), totals(&["-r", BASELINE]));
    assert_eq!(
        totals(&["-r", NO_PART_POWER, "-i"]),
        totals(&["-r", BASELINE, "-i"])
    );
}

#[test]
fn anonymize_keeps_totals() {
    for extra in [&[][..], &["-i"], &["--flatten-site"]] {
        let mut plain = vec!["-r", BASELINE];
        plain.extend(extra);
        let mut anon = plain.clone();
        anon.push("--anonymize");
        assert_eq!(totals(&anon), totals(&plain), "args {plain:?}");
    }
    let (ok, stdout, _) = run(&["-r", BASELINE, "-s", "--anonymize", "--format", "csv"]);
    assert!(ok);
    assert!(!stdout.contains("DC-A"), "names should be hashed");
}

#[test]
fn vm_table_shows_vpartition_value() {
    let (ok, stdout, _) = run(&["-r", BASELINE, "-v"]);
    assert!(ok);
    let row = |vm: &str| {
        stdout
            .lines()
            .find(|l| l.contains(&format!(" {vm} ")))
            .unwrap_or_else(|| panic!("{vm} row missing"))
            .split('│')
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(String::from)
            .collect::<Vec<_>>()
    };
    // [dc, cluster, vm, capacity, vpartition, power]
    assert_eq!(row("vm1")[3..5], ["512.00", "512.00"]);
    assert_eq!(row("vm2")[3..5], ["512.00", "1024.00"]);
    assert_eq!(row("vm3")[3..5], ["2048.00", "-"]);
    assert_eq!(row("vm4")[3..5], ["768.00", "768.00"]);
}

#[test]
fn print_json_capacity_matches_summary() {
    let (ok, stdout, _) = run(&["-r", BASELINE, "--print-json"]);
    assert!(ok);
    let v: Vec<serde_json::Value> = serde_json::from_str(&stdout).unwrap();
    let mib: f64 = v.iter().map(|x| x["capacity"].as_f64().unwrap()).sum();
    assert_eq!(mib / 1048576.0, 3.75);
    assert!(v[0].get("vpartition_capacity").is_none());
}

const DISKS: &str = "testdata/regression_disks.xlsx";

#[test]
fn missing_vdisk_tab_is_graceful_and_unchanged() {
    let (ok, _, stderr) = run(&["-r", BASELINE, "-s", "--format", "csv"]);
    assert!(ok);
    assert!(stderr.contains("vDisk sheet not found"));
    assert_eq!(totals(&["-r", BASELINE]), (3.75, 4));
}

#[test]
fn physical_rdm_excluded_by_default() {
    // vm_vmdk 0.5 + vm_prdm 0.25 + vm_vrdm 0.8 + vm_cap 0.5 = 2.05; vm_only_p excluded
    let (cap, n) = totals(&["-r", DISKS]);
    assert!((cap - 2.05).abs() < 1e-6, "got {cap}");
    assert_eq!(n, 4);
    let (ok, _, stderr) = run(&["-r", DISKS, "-s"]);
    assert!(ok);
    assert!(stderr.contains("physical RDM"));
    assert!(stderr.contains("--verbose") || stderr.contains("--exceptions-file"));
    let (ok, _, verbose) = run(&["-r", DISKS, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("physical_rdm"));
    assert!(verbose.contains("vm_only_p"));
    assert!(
        verbose.contains("all_disks_physical_rdm")
            || verbose.contains("vm_excluded")
            || verbose.contains("no_vadp_eligible")
    );
}

#[test]
fn keep_rdm_includes_physical() {
    // + vm_prdm becomes 2.0 (uncapped guest) and vm_only_p 1.0 → 0.5+2.0+0.8+0.5+1.0 = 4.8
    let (cap, n) = totals(&["-r", DISKS, "--keep-rdm"]);
    assert!((cap - 4.8).abs() < 1e-6, "got {cap}");
    assert_eq!(n, 5);
    let (ok, _, stderr) = run(&["-r", DISKS, "--keep-rdm", "-s"]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("2 physical RDMs kept"), "{stderr}");
    assert!(!stderr.contains("physical RDMs excluded"), "{stderr}");
    assert!(
        stderr.contains("--keep-rdm: +2.75 TB estimate change"),
        "{stderr}"
    );
}

#[test]
fn virtual_rdm_kept_in_estimate() {
    let (ok, _, stderr) = run(&["-r", DISKS, "-s", "--format", "json"]);
    assert!(ok);
    assert!(stderr.contains("physical RDM"));
    let (ok, _, verbose) = run(&["-r", DISKS, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("physical_rdm"));
    assert!(!verbose
        .lines()
        .any(|l| l.contains("vm_vrdm") && l.contains("excluded")));
    let v: serde_json::Value =
        serde_json::from_str(&run(&["-r", DISKS, "-s", "--format", "json"]).1).unwrap();
    let cl2: f64 = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["cluster"] == "CL2")
        .map(|r| r["capacity_tb"].as_f64().unwrap())
        .sum();
    assert!((cl2 - 1.3).abs() < 1e-6, "CL2={cl2}");
}

#[test]
fn partition_capped_at_vdisk() {
    let (ok, _, stderr) = run(&["-r", DISKS, "-s"]);
    assert!(ok);
    assert!(stderr.contains("vPartition capped") || stderr.contains("capped at vDisk"));
    let (ok, _, verbose) = run(&["-r", DISKS, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("vpartition_capped_at_vdisk"));
}

#[test]
fn exceptions_file_csv() {
    let path = std::env::temp_dir().join("rvtools2vse_exceptions_test.csv");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().unwrap();
    let (ok, _, stderr) = run(&[
        "-r",
        DISKS,
        "-s",
        "--format",
        "csv",
        "--exceptions-file",
        path_s,
    ]);
    assert!(ok, "{stderr}");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.starts_with("vm_name,disk,reason,capacity_mib,action\n"));
    assert!(body.contains("physical_rdm"));
    assert!(!body.contains("╭") && !body.contains("│"));
    let _ = std::fs::remove_file(&path);
}

const DISKS2: &str = "testdata/regression_disks2.xlsx";

#[test]
fn independent_disk_excluded_by_default() {
    let (cap, n) = totals(&["-r", DISKS2]);
    // ind 0.5 + share 1.0 + tools 2.0 + nopart 0.5 = 4.0; share_b at 0 still counted
    assert!((cap - 4.0).abs() < 1e-6, "got {cap}");
    assert_eq!(n, 5);
    let (ok, _, stderr) = run(&["-r", DISKS2, "-s"]);
    assert!(ok);
    assert!(stderr.contains("independent disk"));
    let (ok, _, verbose) = run(&["-r", DISKS2, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("independent_disk"));
}

#[test]
fn keep_independent_restores_capacity() {
    let (cap, _) = totals(&["-r", DISKS2, "--keep-independent"]);
    // ind becomes 1.2; +1 +2 +0.5 = 4.7
    assert!((cap - 4.7).abs() < 1e-6, "got {cap}");
}

#[test]
fn shared_disk_counted_once() {
    let (ok, _, stderr) = run(&["-r", DISKS2, "-s", "--format", "json"]);
    assert!(ok);
    assert!(stderr.contains("shared/multi-writer"));
    let (ok, _, verbose) = run(&["-r", DISKS2, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("shared_disk"));
    assert!(verbose.contains("counted_once_on_this_vm"));
    assert!(verbose.contains("deduped_already_counted_on_"));
    let v: serde_json::Value =
        serde_json::from_str(&run(&["-r", DISKS2, "-s", "--format", "json"]).1).unwrap();
    let cl2: f64 = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["cluster"] == "CL2")
        .map(|r| r["capacity_tb"].as_f64().unwrap())
        .sum();
    assert!((cl2 - 1.0).abs() < 1e-6, "CL2 shared total {cl2}");
}

#[test]
fn tools_not_running_falls_back_to_vinfo() {
    let (ok, _, stderr) = run(&["-r", DISKS2, "-s"]);
    assert!(ok);
    assert!(stderr.contains("Tools not running") || stderr.contains("tools"));
    let (ok, _, verbose) = run(&["-r", DISKS2, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("tools_unreliable") || verbose.contains("toolsNotRunning"));
    let v: serde_json::Value =
        serde_json::from_str(&run(&["-r", DISKS2, "-s", "--format", "json"]).1).unwrap();
    let tools_row = v["rows"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["cluster"] == "CL3")
        .unwrap();
    // CL3 = vm_tools 2.0 + vm_nopart 0.5
    assert!((tools_row["capacity_tb"].as_f64().unwrap() - 2.5).abs() < 1e-6);
}

#[test]
fn powered_on_without_vpartition_noted() {
    let (ok, _, stderr) = run(&["-r", DISKS2, "-s"]);
    assert!(ok);
    assert!(stderr.contains("without vPartition") || stderr.contains("no_vpartition"));
    let (ok, _, verbose) = run(&["-r", DISKS2, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("vm_nopart") || verbose.contains("no_vpartition"));
}

#[test]
fn exceptions_file_infers_csv_extension() {
    let path = std::env::temp_dir().join("rvtools2vse_ex_infer.csv");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().unwrap();
    let (ok, _, stderr) = run(&["-r", DISKS, "-s", "--exceptions-file", path_s]);
    assert!(ok, "{stderr}");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.starts_with("vm_name,disk,reason,capacity_mib,action"));
    assert!(!body.contains("╭"));
    // compact summary on stderr by default
    assert!(stderr.contains("exception(s)"));
    assert!(stderr.contains("--verbose"));
    assert!(!stderr.contains("Hard disk 2 | physical_rdm"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn exceptions_file_explicit_format_overrides_extension() {
    let path = std::env::temp_dir().join("rvtools2vse_ex_force.csv");
    let _ = std::fs::remove_file(&path);
    let path_s = path.to_str().unwrap();
    let (ok, _, stderr) = run(&[
        "-r",
        DISKS,
        "-s",
        "--exceptions-file",
        path_s,
        "--format",
        "json",
    ]);
    assert!(ok, "{stderr}");
    let body = std::fs::read_to_string(&path).unwrap();
    assert!(body.trim_start().starts_with('['), "{body}");
    let _ = std::fs::remove_file(&path);
}

#[test]
fn verbose_prints_full_exception_list() {
    let (ok, _, quiet) = run(&["-r", DISKS2, "-s"]);
    assert!(ok);
    assert!(!quiet.contains("independent_disk (independentPersistent)"));
    let (ok, _, verbose) = run(&["-r", DISKS2, "-s", "--verbose"]);
    assert!(ok);
    assert!(verbose.contains("independent_disk (independentPersistent)"));
}

const DISK_SUMMARY: &str =
    "testdata/regression_disk_summary_a.xlsx,testdata/regression_disk_summary_b.xlsx";

#[test]
fn disk_summary_unique_provisioned_sizes_and_actual_impact_across_files() {
    for grouping in ["--flatten", "--flatten-site"] {
        let (excluded, n) = totals(&["-r", DISK_SUMMARY, grouping]);
        let (kept, kept_n) = totals(&["-r", DISK_SUMMARY, grouping, "--keep-independent"]);
        assert_eq!((excluded, n), (1.375, 4));
        assert_eq!((kept, kept_n), (2.125, 5));
        let (ok, _, stderr) = run(&["-r", DISK_SUMMARY, grouping, "-s"]);
        assert!(ok, "{stderr}");
        assert!(
            stderr.contains("4 independent disks excluded (6 attachments; 7.75 TB provisioned)"),
            "{stderr}"
        );
        assert!(
            stderr
                .contains("1 shared/multi-writer disk noted (3 attachments; 4.00 TB provisioned)"),
            "{stderr}"
        );
        assert!(
            stderr.contains("4 VMs with vPartition capped at vDisk (1.00 TB estimate reduction)"),
            "{stderr}"
        );
        assert!(
            stderr.contains(&format!(
                "--keep-independent: {:+.2} TB estimate change",
                kept - excluded
            )),
            "{stderr}"
        );
        assert!(
            stderr.contains("--keep-rdm: +0.00 TB estimate change"),
            "{stderr}"
        );
        assert!(stderr.contains("categories overlap"));
        let (ok, _, stderr) = run(&["-r", DISK_SUMMARY, grouping, "-s", "--keep-independent"]);
        assert!(ok, "{stderr}");
        // The pRDM/independent disk remains excluded by the other policy.
        assert!(
            stderr.contains("3 independent disks kept (5 attachments; 7.00 TB provisioned)"),
            "{stderr}"
        );
        assert!(
            stderr.contains("1 independent disk excluded (1 attachment; 0.75 TB provisioned)"),
            "{stderr}"
        );
        assert!(
            stderr.contains("3 VMs with vPartition capped at vDisk (0.50 TB estimate reduction)"),
            "{stderr}"
        );
        assert!(
            stderr.contains("--keep-independent: +0.75 TB estimate change"),
            "{stderr}"
        );
        assert!(
            stderr.contains("--keep-rdm: +0.25 TB estimate change"),
            "{stderr}"
        );
    }
}

#[test]
fn kept_rdm_summary_respects_other_exclusions_and_exports_schema() {
    let (ok, _, stderr) = run(&["-r", DISK_SUMMARY, "-s", "--keep-rdm"]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("1 physical RDM excluded"), "{stderr}");
    let (ok, _, stderr) = run(&["-r", DISK_SUMMARY, "-s", "--keep-rdm", "--keep-independent"]);
    assert!(ok, "{stderr}");
    assert!(stderr.contains("1 physical RDM kept"), "{stderr}");
    assert!(
        stderr.contains("4 independent disks kept (6 attachments; 7.75 TB provisioned)"),
        "{stderr}"
    );
    assert!(!stderr.contains("independent disk excluded"));
    assert_eq!(
        totals(&["-r", DISK_SUMMARY, "--keep-rdm", "--keep-independent"]),
        (2.375, 5)
    );

    let path = std::env::temp_dir().join("rvtools2vse_disk_summary_details.json");
    let (ok, _, stderr) = run(&[
        "-r",
        DISK_SUMMARY,
        "--exceptions-file",
        path.to_str().unwrap(),
    ]);
    assert!(ok, "{stderr}");
    let rows: Vec<serde_json::Value> =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(
        rows.iter()
            .filter(|r| r["reason"].as_str().unwrap().starts_with("shared_disk"))
            .count(),
        3
    );
    assert!(rows.iter().all(|r| r.as_object().unwrap().len() == 5));
    let floor = rows
        .iter()
        .find(|r| r["vm_name"] == "vm_floor" && r["reason"] == "vpartition_capped_at_vdisk")
        .unwrap();
    assert_eq!(floor["capacity_mib"], 0.0); // raw cap is masked entirely by vInfo
    assert!(floor["action"].as_str().unwrap().contains("guest capped"));
    std::fs::remove_file(path).unwrap();
}
