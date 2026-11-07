# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Docs

- README: added a plain-English "Why some disks are excluded" section covering each exclusion and coverage flag, a worked Coverage example and a glossary, with links from the top of the README, backup coverage and exceptions sections.

## [1.3.0] - 2026-11-07

### Added

- Backup coverage: a stderr `Coverage:` line splits the estimate into image-level (VADP) and "needs an agent or other method" capacity. Kept physical RDMs and independent disks, multi-writer disks and shared-SCSI-bus disks land in the non-VADP group, with a `needs_agent_or_other_method` exception per VM and new `non_vadp_mib` (vms) and `coverage`, `shared_bus`, `raw_lun_id` (disks) parquet columns.
- The coverage block always reports non-VADP storage excluded from the estimate by the current flags ("Not in estimate, needs agent or other method"), with shared disks counted once (Disk UUID, then Raw LUN ID within the vCenter), including multi-writer disks that are also independent.
- `--show-info` / `--summary-file` summaries (table, Markdown, CSV, JSON rows and totals) gain `image_level_tb`, `agent_in_estimate_tb` and `excluded_non_vadp_tb` columns after the existing ones.
- `shared_scsi_bus` exceptions read the vDisk `Shared Bus` column (WSFC-style `physicalSharing` / `virtualSharing`) and group cluster nodes sharing a LUN. Shared physical RDMs without a `Disk UUID` are de-duplicated by `Raw LUN ID` within the vCenter.
- `possible_in_guest_storage (heuristic)` flags guests whose file systems exceed their vDisks by `--guest-gap-threshold` percent (default 10) and `--guest-gap-min-gib` (default 50), after ignoring container-runtime mounts and repeated views of the same file system.
- The `vTools` sheet is read when present (vInfo `Tools` is the fallback), and `vmware_tools` exceptions report `toolsNotInstalled`, `toolsNotRunning` on powered-on VMs and `toolsOld`, with the impact on application-aware processing and guest indexing.
- Cache schema version 2 (adds vTools and the new vDisk/vPartition columns); older cache files are rebuilt automatically.

- `--export-parquet <DIR>` writes `vms.parquet` and `disks.parquet` (cleaned, UUID-joined, filtered tables with a `source_file` column) for pandas/Polars. Honors `--anonymize`.
- `--verbose` prints per-file load time with the cache result (`hit`, `miss`, `refresh`, `bypass`).
- `Containerfile.bench` (rvsum + hyperfine) and README guidance for benchmarking several RVTools files inside a container.
- Automatic compact binary parse cache for the used vInfo, vPartition and vDisk columns, including UUIDs. Sidecar files fall back to the OS cache directory for read-only exports; cache failures never prevent normal parsing.
- `--no-cache` bypasses reads and writes; `--refresh-cache` rebuilds. `--verbose` reports cache activity; normal output stays unchanged. Cache keys validate source path, size, mtime, schema and application version; damaged entries are rebuilt.
- Cache regressions cover output equivalence, option-independent rows, changed inputs, corrupt/versioned entries, read-only fallback and disabled caching. README documents inventory-data privacy, container persistence and a small-fixture timing comparison.

- `duplicate_vm_name` exception category, shown in the compact summary, `--verbose` output and exception files, with actions distinguishing UUID-separated VMs, overlapping exports and ambiguous name-only joins.

### Changed
- vInfo, vPartition and vDisk are now joined per input file by **VM Instance UUID**, then **VM UUID**, then VM name for older exports without UUID columns. Same-named VMs (for example across vCenters or files) are no longer merged, and partitions/disks from one file no longer attach to a same-named VM in another file. Totals can change for inventories with duplicate VM names. Shared-disk deduplication by `Disk UUID` still spans all files.


### Fixed
- `--anonymize` now applies include/exclude filters and `--dc-site-map` entries to the real names before hashing, so filters and site maps work with anonymization.

## [1.2.0] - 2026-11-07

### Changed
- Renamed the crate, library, CLI binary, release assets and container image from `rvtools2vse` to **`rvsum`**. Update scripts to call `rvsum` and use `ghcr.io/shapedthought/rvsum`; command-line flags and output schemas are unchanged.
- Repository links now target `https://github.com/shapedthought/rvsum`. After the GitHub repository is renamed, the old repository URL will redirect. The old `ghcr.io/shapedthought/rvtools2vse` image name is frozen at **1.1.2** and will receive no new releases.
- Rewrote the README with installation, quick-start, sizing rules, a complete grouped CLI reference, output and exception formats, and container/migration guidance.

## [1.1.2] - 2026-11-06

### Fixed
- Disk exception summaries now distinguish kept from excluded disks (including overlapping physical-RDM/independent policies), count unique disk identities across VMs/files, and label sizes as **provisioned**, with attachment counts retained. Overlapping categories are explicitly non-additive; detailed exports retain every attachment/reason and their existing columns.
- Report the actual estimated-capacity change for `--keep-independent` / `--keep-rdm` by comparing the same sizing calculation with just that flag toggled; provisioned disk sizes are no longer presented as estimate changes.
- `vPartition capped at vDisk` capacities now report the reduction that survives the vInfo minimum, not the larger raw guest cap. Counts still identify every VM whose raw guest sum exceeds eligible disk capacity.
- Shared-disk UUID matching ignores surrounding whitespace/case, duplicate rows on the owner count capacity once, and excluded attachments cannot claim ownership ahead of eligible attachments.

### Added
- Synthetic cross-file regression workbooks covering shared independent disks, duplicate attachments, overlapping exclusions, and caps masked by the vInfo minimum.

## [1.1.1] - 2026-11-06

### Fixed
- `--exceptions-file out.csv` (and `--summary-file`) now infer format from the file extension unless `--format` is set, so CSV/JSON files are no longer written as box-drawn tables by default.

### Changed
- Exceptions on stderr are a compact summary by default; `--verbose` prints the full list. The exceptions file always contains the full list.

## [1.1.0] - 2026-11-06

First numbered release after the RVTools summariser refresh (previous crate version was `0.1.0`).

### Added
- Summary output formats for `--show-info`: `--format table|json|csv|md` and `--summary-file` (#2)
- Folder input: `-D/--rvtools-dir` and `--recursive` (#4)
- `Containerfile` and refreshed README (#6)
- Disk awareness phase 1: optional vDisk tab, physical RDM exclusion (`--keep-rdm`), vPartition capped at eligible vDisk capacity, exceptions report (`--exceptions-file`) (#9)
- Disk awareness phase 2: independent disks (`--keep-independent`), shared/multi-writer de-dupe, VMware Tools heuristics (#10)
- Regression test workbooks under `testdata/` and integration tests (#7, #9, #10)
- MIT license, CI workflow, package metadata

### Changed
- Minimum Rust version set to **1.88** (#5)
- RVTools download link updated to the Dell page (#8)
- Tool positioning: general offline RVTools summariser; VSE JSON via `-o`/`-p` is legacy

### Fixed
- Security: upgraded calamine to 0.38 and refreshed transitive deps (zip, mio, anstream, etc.) (#3)
- vPartition without a `Powerstate` column no longer hard-fails (#7)
- `--anonymize` no longer breaks the vInfo/vPartition join (#7)
- `-v` VM table shows the real vPartition capacity (#7)

### Security
- `cargo audit` clean after the calamine/zip upgrades (#3)

[1.1.0]: https://github.com/shapedthought/rvsum/releases/tag/v1.1.0
