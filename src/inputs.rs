//! Resolves the list of RVTools files from `--rvtools-files` and `--rvtools-dir`.

use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{bail, Context, Result};

fn is_candidate(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.starts_with('.') || name.starts_with("~$") {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("xlsx"))
}

fn walk(dir: &Path, recursive: bool, out: &mut Vec<PathBuf>) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("cannot read directory {}", dir.display()))?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let ft = entry.file_type()?;
        let hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if ft.is_dir() {
            if recursive && !hidden {
                walk(&path, recursive, out)?;
            }
        } else if is_candidate(&path) {
            out.push(path);
        }
    }
    Ok(())
}

/// All `.xlsx` files (case-insensitive) in `dir`, skipping hidden files and
/// Excel `~$` lock files, sorted for deterministic order.
pub fn collect_dir(dir: &Path, recursive: bool) -> Result<Vec<PathBuf>> {
    if !dir.is_dir() {
        bail!("RVTools directory not found: {}", dir.display());
    }
    let mut out = Vec::new();
    walk(dir, recursive, &mut out)?;
    out.sort();
    Ok(out)
}

/// Explicit files first (in given order), then directory files; duplicates removed.
pub fn resolve_inputs(
    files: &[PathBuf],
    dirs: &[PathBuf],
    recursive: bool,
) -> Result<Vec<PathBuf>> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    let mut push = |p: PathBuf| {
        let key = fs::canonicalize(&p).unwrap_or_else(|_| p.clone());
        if seen.insert(key) {
            out.push(p);
        }
    };
    files.iter().cloned().for_each(&mut push);
    for dir in dirs {
        let found = collect_dir(dir, recursive)?;
        if found.is_empty() {
            bail!("No .xlsx files found in {}", dir.display());
        }
        found.into_iter().for_each(&mut push);
    }
    if out.is_empty() {
        bail!("No RVTools files specified: use -r/--rvtools-files and/or -D/--rvtools-dir");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(p: &Path) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, b"").unwrap();
    }

    fn setup() -> tempfile::TempDir {
        let t = tempfile::tempdir().unwrap();
        let r = t.path();
        for f in [
            "b.xlsx",
            "a.XLSX",
            "~$a.xlsx",
            ".hidden.xlsx",
            "notes.txt",
            "c.xls",
            "sub/d.xlsx",
            ".git/e.xlsx",
        ] {
            touch(&r.join(f));
        }
        t
    }

    fn names(v: &[PathBuf], root: &Path) -> Vec<String> {
        v.iter()
            .map(|p| p.strip_prefix(root).unwrap().to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn flat_filters_and_sorts() {
        let t = setup();
        let v = collect_dir(t.path(), false).unwrap();
        assert_eq!(names(&v, t.path()), ["a.XLSX", "b.xlsx"]);
    }

    #[test]
    fn recursive_skips_hidden_dirs() {
        let t = setup();
        let v = collect_dir(t.path(), true).unwrap();
        assert_eq!(names(&v, t.path()), ["a.XLSX", "b.xlsx", "sub/d.xlsx"]);
    }

    #[test]
    fn dedupes_files_and_dirs() {
        let t = setup();
        let b = t.path().join("b.xlsx");
        let v = resolve_inputs(std::slice::from_ref(&b), &[t.path().into(), t.path().into()], false).unwrap();
        assert_eq!(names(&v, t.path()), ["b.xlsx", "a.XLSX"]);
    }

    #[test]
    fn errors_when_empty_or_missing() {
        let t = tempfile::tempdir().unwrap();
        let e = resolve_inputs(&[], &[t.path().into()], false).unwrap_err();
        assert!(e.to_string().contains("No .xlsx files found"));
        assert!(resolve_inputs(&[], &[t.path().join("nope")], false).is_err());
        assert!(resolve_inputs(&[], &[], false)
            .unwrap_err()
            .to_string()
            .contains("No RVTools files"));
    }
}
