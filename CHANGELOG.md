# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
