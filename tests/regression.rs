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
    assert_eq!(totals(&["-r", NO_PART_POWER, "-i"]), totals(&["-r", BASELINE, "-i"]));
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
