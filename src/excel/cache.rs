//! Cache the option-independent worksheet rows, never sizing results or filtered VMs.
use calamine::{open_workbook, Data, Range, Reader, Xlsx, XlsxError};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

// Bump whenever the selected columns or their interpretation changes.
const SCHEMA: u32 = 1;
const VERSION: &str = env!("CARGO_PKG_VERSION");
const MAGIC: &[u8] = b"RVSUMCACHE\x01";
const MAX_BYTES: u64 = 512 * 1024 * 1024;
const SHEETS: [&str; 3] = ["vInfo", "vPartition", "vDisk"];

pub(super) struct Workbook {
    sheets: [Option<Range<Data>>; 3],
}

impl Workbook {
    pub(super) fn worksheet_range(&mut self, name: &str) -> Result<Range<Data>, XlsxError> {
        SHEETS
            .iter()
            .position(|s| *s == name)
            .and_then(|i| self.sheets[i].take())
            .ok_or_else(|| XlsxError::WorksheetNotFound(name.to_string()))
    }
}

#[derive(Serialize, Deserialize, PartialEq)]
struct Fingerprint {
    source: PathBuf,
    size: u64,
    seconds: u64,
    nanos: u32,
}

impl Fingerprint {
    fn read(path: &Path) -> Option<Self> {
        let metadata = fs::metadata(path).ok()?;
        let modified = metadata.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
        Some(Self {
            source: fs::canonicalize(path).ok()?,
            size: metadata.len(),
            seconds: modified.as_secs(),
            nanos: modified.subsec_nanos(),
        })
    }
}

#[derive(Serialize, Deserialize)]
struct Entry {
    schema: u32,
    version: String,
    fingerprint: Fingerprint,
    sheets: [Option<Sheet>; 3],
}

#[derive(Serialize, Deserialize)]
struct Sheet {
    width: usize,
    cells: Vec<Cell>,
}

// Unsupported cell kinds in a selected column disable caching, not workbook loading.
// This preserves calamine's original validation/error behaviour without a lossy codec.
#[derive(Serialize, Deserialize)]
enum Cell {
    Empty,
    String(String),
    Float(f64),
    Int(i64),
    Bool(bool),
}

impl Sheet {
    fn encode(range: &Range<Data>) -> Option<Self> {
        let width = range.width();
        let cells = range
            .rows()
            .flatten()
            .map(|cell| {
                Some(match cell {
                    Data::Empty => Cell::Empty,
                    Data::String(s) => Cell::String(s.clone()),
                    Data::Float(f) => Cell::Float(*f),
                    Data::Int(i) => Cell::Int(*i),
                    Data::Bool(b) => Cell::Bool(*b),
                    _ => return None,
                })
            })
            .collect::<Option<Vec<_>>>()?;
        Some(Self { width, cells })
    }

    fn decode(self) -> Option<Range<Data>> {
        if self.width == 0 {
            return self.cells.is_empty().then(Range::empty);
        }
        if self.cells.is_empty() || !self.cells.len().is_multiple_of(self.width) {
            return None;
        }
        let height = self.cells.len() / self.width;
        let end = (
            u32::try_from(height - 1).ok()?,
            u32::try_from(self.width - 1).ok()?,
        );
        let mut range = Range::new((0, 0), end);
        for (i, cell) in self.cells.into_iter().enumerate() {
            let data = match cell {
                Cell::Empty => Data::Empty,
                Cell::String(s) => Data::String(s),
                Cell::Float(f) => Data::Float(f),
                Cell::Int(i) => Data::Int(i),
                Cell::Bool(b) => Data::Bool(b),
            };
            range.set_value(((i / self.width) as u32, (i % self.width) as u32), data);
        }
        Some(range)
    }
}

/// Keep all rows (including powered-off VMs), both modern and legacy capacity
/// columns, and both UUID schemes. Column selection is independent of CLI options.
fn project(range: Range<Data>) -> Range<Data> {
    const COLUMNS: &[&str] = &[
        "VM",
        "Powerstate",
        "In Use MB",
        "In Use MiB",
        "Datacenter",
        "Cluster",
        "Tools",
        "Consumed MB",
        "Consumed MiB",
        "Capacity MB",
        "Capacity MiB",
        "Raw Comp. Mode",
        "Disk",
        "Disk Mode",
        "Mode",
        "Sharing",
        "Sharing mode",
        "Disk UUID",
        "VM Instance UUID",
        "Instance UUID",
        "InstanceUUID",
        "VM UUID",
        "SMBIOS UUID",
        "BIOS UUID",
        "UUID",
    ];
    let Some(header) = range.rows().next() else {
        return range;
    };
    let mut columns: Vec<usize> = header
        .iter()
        .enumerate()
        .filter_map(|(i, c)| match c {
            Data::String(s) if COLUMNS.iter().any(|h| s.trim().eq_ignore_ascii_case(h)) => Some(i),
            _ => None,
        })
        .collect();
    // Preserve a nonempty header even when none of the required columns exist.
    if columns.is_empty() {
        columns.push(0);
    }
    let mut projected = Range::new(
        (0, 0),
        ((range.height() - 1) as u32, (columns.len() - 1) as u32),
    );
    for (r, row) in range.rows().enumerate() {
        for (c, original) in columns.iter().enumerate() {
            projected.set_value((r as u32, c as u32), row[*original].clone());
        }
    }
    projected
}

fn sidecar(path: &Path) -> Option<PathBuf> {
    let mut name = std::ffi::OsString::from(".");
    name.push(path.file_name()?);
    name.push(".rvsum-cache");
    Some(path.with_file_name(name))
}

fn fallback(fingerprint: &Fingerprint) -> Option<PathBuf> {
    let source = postcard::to_allocvec(&fingerprint.source).ok()?;
    let key = format!("{:x}", Sha256::digest(source));
    Some(
        dirs::cache_dir()?
            .join("rvsum")
            .join(format!("{key}.rvsum-cache")),
    )
}

fn read(path: &Path, fingerprint: &Fingerprint) -> Option<Workbook> {
    let file = File::open(path).ok()?;
    if file.metadata().ok()?.len() > MAX_BYTES {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let data = bytes.strip_prefix(MAGIC)?;
    let (checksum, payload) = data.split_at_checked(32)?;
    if Sha256::digest(payload).as_slice() != checksum {
        return None;
    }
    let (entry, remaining) = postcard::take_from_bytes::<Entry>(payload).ok()?;
    if !remaining.is_empty()
        || entry.schema != SCHEMA
        || entry.version != VERSION
        || entry.fingerprint != *fingerprint
    {
        return None;
    }
    let mut sheets = [None, None, None];
    for (i, sheet) in entry.sheets.into_iter().enumerate() {
        sheets[i] = match sheet {
            Some(sheet) => Some(sheet.decode()?),
            None => None,
        };
    }
    Some(Workbook { sheets })
}

fn write(path: &Path, bytes: &[u8], create_parent: bool) -> std::io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    if create_parent {
        fs::create_dir_all(parent)?;
    }
    // Atomic replacement avoids partial caches and following an existing symlink.
    // NamedTempFile is owner-only (0600) on Unix: caches contain real inventory names.
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.persist(path)?;
    Ok(())
}

pub(super) fn load(
    path: &Path,
    no_cache: bool,
    refresh: bool,
    verbose: bool,
) -> Result<Workbook, XlsxError> {
    let started = std::time::Instant::now();
    let (workbook, status) = load_inner(path, no_cache, refresh, verbose)?;
    if verbose {
        eprintln!(
            "Loaded {} in {:.1} ms (cache {status})",
            path.display(),
            started.elapsed().as_secs_f64() * 1000.0
        );
    }
    Ok(workbook)
}

fn load_inner(
    path: &Path,
    no_cache: bool,
    refresh: bool,
    verbose: bool,
) -> Result<(Workbook, &'static str), XlsxError> {
    let fingerprint = (!no_cache).then(|| Fingerprint::read(path)).flatten();
    let paths = fingerprint.as_ref().map(|f| (sidecar(path), fallback(f)));
    if !refresh {
        if let (Some(fingerprint), Some((sidecar, fallback))) = (&fingerprint, &paths) {
            for candidate in [sidecar, fallback].into_iter().flatten() {
                if let Some(workbook) = read(candidate, fingerprint) {
                    if verbose {
                        eprintln!("Cache hit: {} ({})", path.display(), candidate.display());
                    }
                    return Ok((workbook, "hit"));
                }
            }
        }
    }
    if verbose {
        let reason = if no_cache {
            "bypass (--no-cache)"
        } else if refresh {
            "refresh"
        } else {
            "miss"
        };
        eprintln!("Cache {reason}: {}", path.display());
    }
    let status = if no_cache {
        "bypass"
    } else if refresh {
        "refresh"
    } else {
        "miss"
    };
    let mut excel: Xlsx<_> = open_workbook(path)?;
    let mut sheets = [None, None, None];
    for (i, name) in SHEETS.iter().enumerate() {
        sheets[i] = match excel.worksheet_range(name) {
            Ok(range) => Some(project(range)),
            Err(XlsxError::WorksheetNotFound(_)) => None,
            Err(e) => return Err(e),
        };
    }
    let workbook = Workbook { sheets };
    if let (Some(fingerprint), Some((sidecar, fallback))) = (fingerprint, paths) {
        // Do not cache a source changed while calamine was reading it.
        if Fingerprint::read(path).as_ref() == Some(&fingerprint) {
            let encoded: Option<Vec<Option<Sheet>>> = workbook
                .sheets
                .iter()
                .map(|sheet| match sheet {
                    Some(sheet) => Sheet::encode(sheet).map(Some),
                    None => Some(None),
                })
                .collect();
            if let Some(encoded) = encoded {
                let entry = Entry {
                    schema: SCHEMA,
                    version: VERSION.to_string(),
                    fingerprint,
                    sheets: encoded.try_into().ok().unwrap(),
                };
                if let Ok(payload) = postcard::to_allocvec(&entry) {
                    let mut bytes = MAGIC.to_vec();
                    bytes.extend_from_slice(&Sha256::digest(&payload));
                    bytes.extend_from_slice(&payload);
                    if bytes.len() as u64 <= MAX_BYTES {
                        let stored = sidecar
                            .as_ref()
                            .is_some_and(|p| write(p, &bytes, false).is_ok())
                            || fallback
                                .as_ref()
                                .is_some_and(|p| write(p, &bytes, true).is_ok());
                        if verbose && !stored {
                            eprintln!(
                                "Cache unavailable: {}; continuing without saving",
                                path.display()
                            );
                        }
                    }
                }
            }
        }
    }
    Ok((workbook, status))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_and_version_mismatch_are_cache_misses() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.xlsx");
        fs::write(&source, b"source").unwrap();
        let path = sidecar(&source).unwrap();
        for (schema, version) in [(SCHEMA + 1, VERSION), (SCHEMA, "old-version")] {
            let fingerprint = Fingerprint::read(&source).unwrap();
            let entry = Entry {
                schema,
                version: version.to_string(),
                fingerprint: Fingerprint::read(&source).unwrap(),
                sheets: [None, None, None],
            };
            let payload = postcard::to_allocvec(&entry).unwrap();
            let mut bytes = MAGIC.to_vec();
            bytes.extend_from_slice(&Sha256::digest(&payload));
            bytes.extend(payload);
            write(&path, &bytes, false).unwrap();
            assert!(read(&path, &fingerprint).is_none());
        }
    }

    #[test]
    fn checksum_rejects_modified_payload_and_invalid_shape_is_safe() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.xlsx");
        fs::write(&source, b"source").unwrap();
        let fingerprint = Fingerprint::read(&source).unwrap();
        let path = sidecar(&source).unwrap();
        let mut bytes = MAGIC.to_vec();
        bytes.extend_from_slice(&[0; 32]);
        bytes.extend_from_slice(b"damaged payload");
        write(&path, &bytes, false).unwrap();
        assert!(read(&path, &fingerprint).is_none());
        assert!(Sheet {
            width: usize::MAX,
            cells: vec![Cell::Empty]
        }
        .decode()
        .is_none());
    }
}
