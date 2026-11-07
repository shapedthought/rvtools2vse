//! `--export-parquet` round trip.
use std::{fs, path::Path, process::Command};

use arrow_array::{Array, BooleanArray, RecordBatch, StringArray};
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use tempfile::TempDir;

fn read(path: &Path) -> RecordBatch {
    let file = fs::File::open(path).unwrap();
    let reader = ParquetRecordBatchReaderBuilder::try_new(file)
        .unwrap()
        .build()
        .unwrap();
    let batches: Vec<RecordBatch> = reader.map(|b| b.unwrap()).collect();
    arrow_select_concat(&batches)
}

fn arrow_select_concat(batches: &[RecordBatch]) -> RecordBatch {
    assert_eq!(batches.len(), 1, "small fixtures fit one batch");
    batches[0].clone()
}

fn strings(batch: &RecordBatch, col: &str) -> Vec<String> {
    let a = batch
        .column_by_name(col)
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    (0..a.len()).map(|i| a.value(i).to_string()).collect()
}

fn run(dir: &TempDir, flags: &[&str]) -> (String, std::path::PathBuf) {
    let input = dir.path().join("input.xlsx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_uuid_a.xlsx"),
        &input,
    )
    .unwrap();
    let out_dir = dir.path().join("pq");
    let out = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(&input)
        .arg("--export-parquet")
        .arg(&out_dir)
        .arg("--no-cache")
        .args(flags)
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    assert!(out.status.success(), "{stderr}");
    assert!(stderr.contains("Parquet written to:"), "{stderr}");
    (String::from_utf8(out.stdout).unwrap(), out_dir)
}

#[test]
fn round_trip_matches_summary() {
    let dir = TempDir::new().unwrap();
    let (stdout, out) = run(&dir, &[]);
    let vms = read(&out.join("vms.parquet"));
    let total: usize = stdout
        .lines()
        .find_map(|l| l.strip_prefix("Total VMs: "))
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(vms.num_rows(), total);
    assert!(strings(&vms, "source_file")
        .iter()
        .all(|s| s == "input.xlsx"));
    let disks = read(&out.join("disks.parquet"));
    assert!(disks.num_rows() > 0);
    let names = strings(&vms, "vm_name");
    for n in strings(&disks, "vm_name") {
        assert!(names.contains(&n), "disk VM {n} not in VM table");
    }
    assert!(disks
        .column_by_name("excluded_by_flags")
        .unwrap()
        .as_any()
        .downcast_ref::<BooleanArray>()
        .is_some());
}

#[test]
fn anonymize_hashes_names_and_filters_apply() {
    let plain_dir = TempDir::new().unwrap();
    let (_, plain) = run(&plain_dir, &[]);
    let plain_names = strings(&read(&plain.join("vms.parquet")), "vm_name");
    let victim = plain_names[0].clone();

    let dir = TempDir::new().unwrap();
    let (_, out) = run(&dir, &["--anonymize"]);
    let vms = read(&out.join("vms.parquet"));
    let names = strings(&vms, "vm_name");
    assert_eq!(names.len(), plain_names.len());
    assert!(names.iter().all(|n| !plain_names.contains(n)));
    assert!(strings(&vms, "source_file")
        .iter()
        .all(|s| s != "input.xlsx"));

    let dir = TempDir::new().unwrap();
    let (_, out) = run(&dir, &["--vm-exclude", &victim]);
    let names = strings(&read(&out.join("vms.parquet")), "vm_name");
    assert!(!names.contains(&victim));
    assert!(strings(&read(&out.join("disks.parquet")), "vm_name")
        .iter()
        .all(|n| n != &victim));
}

#[test]
fn verbose_reports_per_file_load_time() {
    let dir = TempDir::new().unwrap();
    let input = dir.path().join("input.xlsx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_uuid_a.xlsx"),
        &input,
    )
    .unwrap();
    let go = || {
        let out = Command::new(env!("CARGO_BIN_EXE_rvsum"))
            .arg("-r")
            .arg(&input)
            .arg("--verbose")
            .env("XDG_CACHE_HOME", dir.path().join("c"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stderr).to_string()
    };
    assert!(go().contains("ms (cache miss)"));
    assert!(go().contains("ms (cache hit)"));
}
