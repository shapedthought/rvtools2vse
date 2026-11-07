//! The cache stores raw rows, so options may change freely between runs.
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn fixture(dir: &TempDir, name: &str) -> std::path::PathBuf {
    let dest = dir.path().join("input.xlsx");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("testdata")
            .join(name),
        &dest,
    )
    .unwrap();
    dest
}

fn run(path: &Path, cache: &Path, flags: &[&str]) -> (String, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_rvsum"))
        .arg("-r")
        .arg(path)
        .args(["-s", "--format", "json"])
        .args(flags)
        .env("XDG_CACHE_HOME", cache)
        .output()
        .unwrap();
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(out.status.success(), "{stderr}");
    (String::from_utf8(out.stdout).unwrap(), stderr)
}

fn sidecar(path: &Path) -> std::path::PathBuf {
    path.with_file_name(".input.xlsx.rvsum-cache")
}

#[test]
fn hit_matches_miss_and_options_are_applied_after_loading() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_uuid_a.xlsx");
    let cache = dir.path().join("os-cache");
    let (miss, err) = run(&path, &cache, &["--verbose"]);
    assert!(err.contains("Cache miss:"), "{err}");
    assert!(sidecar(&path).is_file());
    let (hit, err) = run(&path, &cache, &["--verbose"]);
    assert!(err.contains("Cache hit:"), "{err}");
    assert_eq!(hit, miss);
    // UUID-separated names, filters, policies and anonymization must not be baked in.
    for flags in [
        vec!["--anonymize"],
        vec!["--vm-exclude", "web01"],
        vec!["--dc-include", "DC-A"],
        vec!["--keep-independent", "--keep-rdm", "-i"],
        vec!["-d", "--flatten-site"],
    ] {
        let cached = run(&path, &cache, &flags);
        let mut bypass = flags;
        bypass.push("--no-cache");
        assert_eq!(cached, run(&path, &cache, &bypass));
    }
    // A cache populated with restrictive options must still include every row.
    run(
        &path,
        &cache,
        &[
            "--refresh-cache",
            "--vm-exclude",
            "web01",
            "--anonymize",
            "-d",
        ],
    );
    assert_eq!(run(&path, &cache, &[]).0, miss);
}

#[test]
fn powered_off_and_partition_rows_survive_restrictive_first_run() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_baseline.xlsx");
    let cache = dir.path().join("os-cache");
    run(&path, &cache, &["-d", "--dc-include", "DC-A"]);
    for flags in [vec![], vec!["-i"], vec!["-d"]] {
        let expected = run(&path, &cache, &flags).0;
        let mut bypass = flags;
        bypass.push("--no-cache");
        assert_eq!(expected, run(&path, &cache, &bypass).0);
    }
}

#[test]
fn source_change_invalidates_and_corruption_is_rebuilt() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_baseline.xlsx");
    let cache = dir.path().join("os-cache");
    let initial = run(&path, &cache, &[]).0;
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("testdata/regression_disks2.xlsx"),
        &path,
    )
    .unwrap();
    let (changed, err) = run(&path, &cache, &["--verbose"]);
    assert!(err.contains("Cache miss:"), "{err}");
    assert_ne!(initial, changed);
    assert_eq!(changed, run(&path, &cache, &["--no-cache"]).0);
    fs::write(sidecar(&path), b"not a cache").unwrap();
    let (rebuilt, err) = run(&path, &cache, &["--verbose"]);
    assert_eq!(rebuilt, changed);
    assert!(err.contains("Cache miss:"));
    assert!(run(&path, &cache, &["--verbose"]).1.contains("Cache hit:"));
}

#[test]
fn mtime_alone_invalidates() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_baseline.xlsx");
    let cache = dir.path().join("os-cache");
    let expected = run(&path, &cache, &[]).0;
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let later = modified + std::time::Duration::from_secs(2);
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(later))
        .unwrap();
    let (actual, err) = run(&path, &cache, &["--verbose"]);
    assert_eq!(actual, expected);
    assert!(err.contains("Cache miss:"), "{err}");
}

#[test]
fn no_cache_bypasses_reads_and_writes_and_refresh_rebuilds() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_baseline.xlsx");
    let cache = dir.path().join("os-cache");
    let (expected, err) = run(&path, &cache, &["--no-cache", "--verbose"]);
    assert!(err.contains("Cache bypass"));
    assert!(!sidecar(&path).exists());
    assert!(!cache.exists());
    run(&path, &cache, &[]);
    fs::write(sidecar(&path), b"leave this alone").unwrap();
    assert_eq!(expected, run(&path, &cache, &["--no-cache"]).0);
    assert_eq!(fs::read(sidecar(&path)).unwrap(), b"leave this alone");
    let (actual, err) = run(&path, &cache, &["--refresh-cache", "--verbose"]);
    assert_eq!(actual, expected);
    assert!(err.contains("Cache refresh:"));
    assert!(run(&path, &cache, &["--verbose"]).1.contains("Cache hit:"));
    assert!(!run(&path, &cache, &[]).1.contains("Cache "));
}

#[cfg(target_os = "linux")]
#[test]
fn read_only_source_directory_falls_back_to_os_cache() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let source = TempDir::new().unwrap();
    let path = fixture(&source, "regression_baseline.xlsx");
    let cache = dir.path().join("os-cache");
    fs::set_permissions(source.path(), fs::Permissions::from_mode(0o555)).unwrap();
    // This test is run unprivileged in CI; root bypasses filesystem permissions.
    if fs::write(source.path().join("probe"), b"").is_ok() {
        fs::set_permissions(source.path(), fs::Permissions::from_mode(0o755)).unwrap();
        return;
    }
    let first = run(&path, &cache, &["--verbose"]);
    let second = run(&path, &cache, &["--verbose"]);
    fs::set_permissions(source.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(first.0, second.0);
    assert!(!sidecar(&path).exists());
    assert!(first.1.contains("Cache miss:"));
    assert!(second.1.contains("Cache hit:"), "{}", second.1);
    let entries: Vec<_> = fs::read_dir(cache.join("rvsum")).unwrap().collect();
    assert_eq!(entries.len(), 1);
    assert_eq!(
        fs::metadata(entries[0].as_ref().unwrap().path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[cfg(target_os = "linux")]
#[test]
fn unwritable_cache_locations_are_nonfatal() {
    let dir = TempDir::new().unwrap();
    let path = fixture(&dir, "regression_baseline.xlsx");
    // A directory at the sidecar path cannot be replaced with a cache file.
    fs::create_dir(sidecar(&path)).unwrap();
    let cache = dir.path().join("cache-is-a-file");
    fs::write(&cache, b"").unwrap();
    let (actual, err) = run(&path, &cache, &["--verbose"]);
    assert!(err.contains("Cache unavailable:"));
    assert_eq!(actual, run(&path, &cache, &["--no-cache"]).0);
}
