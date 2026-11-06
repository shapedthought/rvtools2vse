# rvsum

**Quick, offline summaries of RVTools exports for backup sizing.** Point rvsum at one workbook or a folder of exports to get VM counts and estimated source capacity by datacenter and cluster, without opening a spreadsheet.

rvsum reads the `vInfo`, optional `vPartition` and optional `vDisk` sheets in [RVTools `.xlsx` exports](https://www.dell.com/support/kbdoc/en-gb/000325532/rvtools-4-7-1-installer). RVTools is owned by Dell (previously Robware). All processing is local: no vCenter connection or upload is needed, and input workbooks are not modified.

- Terminal tables, CSV for spreadsheets, JSON for scripts, and Markdown for reports.
- Several files or folders in one run, with optional recursive discovery.
- Powered-off VM and name filters, site roll-ups, and VADP-oriented disk diagnostics.

[Install](#install) · [Quick start](#quick-start) · [Capacity rules](#how-capacity-is-calculated) · [Outputs](#output-formats) · [Podman](#podman-workflow) · [Flags](#full-flag-reference) · [Migration](#migrating-from-rvtools2vse)

> This is an **estimate of source data**, not a backup repository sizing calculator or a guarantee of backup support. It does not model retention, change rate, compression or deduplication for backup storage. Review the [capacity rules](#how-capacity-is-calculated) and [limitations](#known-limitations) before using the totals.

## Install

The new binary and image names apply from the first **rvsum** release. Release **1.1.2 and earlier** still use `rvtools2vse`; if the new assets are not published yet, build from source. See [migration](#migrating-from-rvtools2vse) and the [changelog](CHANGELOG.md).

### Release binaries

Download the binary and its `.sha256` checksum from [GitHub Releases](https://github.com/shapedthought/rvsum/releases):

| Platform | Asset |
| --- | --- |
| Linux x86-64 | `rvsum-linux-x86_64` |
| Linux ARM64 | `rvsum-linux-aarch64` |
| macOS Intel | `rvsum-macos-x86_64` |
| macOS Apple Silicon | `rvsum-macos-aarch64` |
| Windows x86-64 | `rvsum-windows-x86_64.exe` |

Check the SHA-256 against the companion file. On Linux/macOS, make the download executable and put it on your PATH, for example:

```sh
chmod +x rvsum-linux-x86_64
mkdir -p "$HOME/.local/bin"
cp rvsum-linux-x86_64 "$HOME/.local/bin/rvsum"
# Ensure $HOME/.local/bin is on your PATH.
rvsum --version
```

On Windows, rename the download to `rvsum.exe` and put its folder on PATH (or run `./rvsum.exe` from that folder in PowerShell). Linux binaries use glibc; use the container if your distribution's glibc is too old.

### Container (Podman or Docker)

The GHCR image supports **Linux amd64 and arm64**. Use `latest`, or pin a published version tag for repeatable runs. Docker can be used in place of Podman.

```sh
podman pull ghcr.io/shapedthought/rvsum:latest
podman run --rm ghcr.io/shapedthought/rvsum:latest --version
podman run --rm -v "$PWD:/data" -w /data ghcr.io/shapedthought/rvsum:latest -D . -s
```

See the [container workflow](#podman-workflow) for repeated runs without recreating the container each time.

### Build with Cargo

Requires **Rust 1.88 or newer**. No crates.io publication is assumed; build from this repository:

```sh
git clone https://github.com/shapedthought/rvsum.git
cd rvsum
cargo build --release --locked
./target/release/rvsum --version
# Optional: install the binary on your Cargo PATH.
cargo install --path . --locked
```

Uninstall a Cargo-installed copy with `cargo uninstall rvsum`.

## Quick start

```sh
# Summarise one export (powered-off VMs excluded by default).
rvsum -r rvtools.xlsx -s

# Combine every workbook in a folder, with one row per datacenter.
rvsum -D ./exports -s --flatten-site

# Write spreadsheet-friendly summaries and the full exceptions list.
rvsum -D ./exports --summary-file summary.csv --exceptions-file exceptions.csv

# Include powered-off VMs and omit a test cluster.
rvsum -D ./exports -i --cluster-exclude test-cluster -s

# A machine-readable grand total.
rvsum -D ./exports --flatten -s --format json | jq '.totals'
```

The default summary is a table of `Datacenter`, `Cluster`, `Capacity (TB)` and `VM Count`, largest capacity first. Human-readable runs also print total VMs, total capacity and average VM size. Disk exceptions go to **stderr**, separate from the report on **stdout**.

## Input files and folders

At least one `-r/--rvtools-files` or `-D/--rvtools-dir` input is required, except for help, version and site-map template generation.

```sh
rvsum -r "site a.xlsx",site-b.xlsx -s
rvsum -D ./site-a,./site-b --recursive -s
rvsum -r standalone.xlsx -D ./exports -s
```

- `-r` and `-D` accept comma-separated lists and can be combined. Quote paths containing spaces.
- Folder discovery matches `.xlsx` case-insensitively, skips hidden files and Excel `~$` lock files, and sorts discovered paths. `--recursive` also searches subfolders, excluding hidden ones.
- Duplicate file paths are only read once. Empty or missing input folders are errors. The discovered file list is printed to stderr.
- For exports **older than RVTools v4.1.2**, add `--legacy` to read the `MB` rather than `MiB` column names.

Sheet and column names must match exactly:

| Sheet | Required columns | Optional columns used |
| --- | --- | --- |
| `vInfo` | `VM`, `Powerstate`, `Datacenter`, `Cluster`, `In Use MiB` | `Tools` |
| `vPartition` (optional sheet) | `VM`, `Consumed MiB` | `Powerstate` |
| `vDisk` (optional sheet) | `VM`, `Capacity MiB` | `Raw Comp. Mode`, `Disk Mode` (or `Mode`), `Sharing` (or `Sharing mode`), `Disk UUID`, `Disk`, `Powerstate` |

`--legacy` uses `In Use MB`, `Consumed MB` and `Capacity MB`. A missing `vPartition` or `vDisk` sheet produces a warning and a fallback; missing optional disk columns reduce what rvsum can detect. Without vPartition `Powerstate`, the included vInfo VM population controls which partitions contribute. Empty datacenter or cluster cells are labelled `None`.

## How capacity is calculated

Calculations happen per included VM, before grouping into datacenters/clusters/sites. Powered-off VMs are excluded by default; `-i` includes them.

1. Start with **vInfo `In Use MiB`**.
2. Sum the VM's **vPartition `Consumed MiB`** values, when usable guest data exists.
3. If that VM has vDisk rows, cap the guest sum at the total **eligible vDisk `Capacity MiB`**, after the disk policies below.
4. Use the **lower of vInfo and the resulting guest sum**.

In the normal case with all three sources, this is:

```text
VM capacity = min(vInfo In Use, sum(vPartition Consumed), eligible vDisk capacity)
```

The vDisk term is omitted when that VM has no disk rows. With `-d/--do-not-use-vpartition`, missing partitions, or unreliable Tools, rvsum falls back to **vInfo**: it does **not** apply the vDisk cap on that fallback path, to retain vInfo's swap/snapshot accounting. Disk eligibility still excludes VMs with only excluded disks, and shared-only duplicate attachments can leave a VM at zero.

The report's `TB` is **binary** (MiB / 1,048,576, i.e. TiB); average `GB` is also binary (GiB). Values in tables/CSV/Markdown are rounded to two decimals. JSON summary capacities are unrounded.

### How VMs are matched across tabs

vPartition and vDisk rows are joined to a vInfo record **within the same input file**, never across files, using the first identity available:

1. **VM Instance UUID** (`VM Instance UUID` / `Instance UUID` column), then
2. **VM UUID** (`VM UUID` / `SMBIOS UUID` / `BIOS UUID` column), then
3. **VM name**, for older exports without UUID columns or with blank values.

Column names are matched case-insensitively and each tab is checked independently, so a vInfo tab with both UUIDs still joins a vDisk tab that only has `VM UUID`. A child row whose UUID is not in that file's vInfo (for example a filtered-out VM) is ignored instead of being attached to another VM with the same name. Shared-disk deduplication still uses `Disk UUID` across **all** files, so one multi-writer disk seen from two vCenter exports is counted once.

When the same VM name appears on more than one vInfo record, each record is reported as a `duplicate_vm_name` exception:

| Action | Meaning |
| --- | --- |
| `joined_separately_by_uuid` | Different VMs that happen to share a name (common across vCenters); each is sized from its own rows. |
| `same_uuid_in_multiple_records_check_overlapping_exports` | The same VM appears in several files, e.g. overlapping exports; it is counted once per record, so remove the duplicate export. |
| `no_uuid_joined_by_name_ambiguous` | No UUID is available; partition/disk rows attach to the first record with that name in the file. |

### Disk policy and VMware Tools

| Condition | Default behaviour | Override / caveat |
| --- | --- | --- |
| Physical-compatibility RDM (`physicalMode`) | Excluded from eligible disk capacity | `--keep-rdm` includes it in the estimate, not in snapshot support. Virtual-compatibility RDMs are kept. |
| Independent disk (`independentPersistent` / `independentNonPersistent`) | Excluded from eligible disk capacity | `--keep-independent` includes it in the estimate, not in snapshot support. |
| Shared / multi-writer disk | Eligible provisioned capacity counted once, allocated to the first eligible VM in name order | Identified by `Disk UUID` (case/whitespace normalised), or disk label plus size if the UUID is missing. Duplicate rows on the owning VM are also deduplicated. |
| VM with only policy-excluded disks | VM omitted from capacity **and VM count** | A VM with only shared disks already owned elsewhere remains in the count at zero capacity. |
| vInfo `Tools` = `toolsNotInstalled` / `toolsNotRunning` | Ignore guest partition data and fall back to vInfo | `toolsOld` and unknown/missing Tools states do not force this fallback. |
| Powered-on VM without vPartition rows | Use vInfo and report a diagnostic | This can be stale/missing guest information; inspect before trusting the estimate. |

A disk can be both physical RDM and independent: **both** keep flags are needed to override both exclusion policies. With partial disk exclusions and no usable guest data, the vInfo fallback is not reduced proportionally. Shared-disk deduplication limits eligible disk capacity; it cannot allocate guest usage precisely and does not fully correct a mixed-disk VM on the vInfo fallback path.

## Exceptions: summary, verbose and files

By default, stderr shows **counts by reason**, not thousands of individual rows. For disk categories it distinguishes **kept** from **excluded**, counts unique disk identities across the inputs, and gives attachment counts and **provisioned** TB. Categories overlap: for example a shared independent disk appears in both. Those sizes are **not additive and are not changes to the estimate**.

Where applicable, a separate `--keep-independent` / `--keep-rdm` line shows the signed **estimate change, keeping vs excluding**, rerunning the same calculation with only that flag toggled. This includes the effects of vInfo, guest usage, Tools fallback, VM exclusions and shared ownership. It is calculated **before site mapping**, which may omit or duplicate datacenters. `--flatten` and `--flatten-site` preserve it. Different flags' changes are not additive; changing shared-disk ownership can even produce a negative change.

`vPartition capped at vDisk` counts VMs whose raw guest sum exceeded eligible disk capacity. Its TB value is the **actual estimate reduction after the vInfo minimum**; that reduction can be zero.

```sh
# Compact reason counts on stderr (the default).
rvsum -D ./exports -s

# Full per-VM/per-disk exception list on stderr.
rvsum -D ./exports -s --verbose

# Always write every exception row, independently of verbosity.
rvsum -D ./exports -s --exceptions-file exceptions.csv
```

**`--verbose` has no short alias. `-v` is still `--vm-table-print`.**

Exception files use `vm_name,disk,reason,capacity_mib,action` (JSON is an array of these objects). CSV is plain CSV with a header and one row per exception, not a box-drawn table. For disk reasons, `capacity_mib` is **provisioned capacity per attachment/reason**: summing all rows double-counts shared/overlapping categories. For `vpartition_capped_at_vdisk` it is the actual estimate reduction; fallback rows are diagnostics, not reductions. Use the estimate-change line, not a sum of exception rows, to compare keep flags.

## Output formats

`-s/--show-info` prints the grouped summary. `--summary-file PATH` writes it to a file instead and implies `-s`. `--exceptions-file PATH` writes the full exceptions list. Both file paths are used **exactly as supplied**, with no extension added.

| Format | Summary output |
| --- | --- |
| `table` | Box-drawn terminal table; the default on stdout. |
| `csv` | Header `datacenter,cluster,capacity_tb,vm_count`, then one row per group. |
| `json` | Object with `rows` (`datacenter`, `cluster`, `capacity_tb`, `vm_count`) and `totals` (`capacity_tb`, `vm_count`). |
| `md` | GitHub-flavoured Markdown table with a final **Total** row. |

**Format selection:**

1. An explicit `--format table|json|csv|md` wins for **both** summary and exception outputs.
2. Otherwise, each output file independently infers its format from its extension: `.csv`, `.json`, `.md` / `.markdown` (case-insensitive). Any other extension, or none, means `table`.
3. Stdout defaults to `table`; use `--format` for a different stdout format.

```sh
# Different inferred formats in the same run.
rvsum -D ./exports --summary-file summary.json --exceptions-file exceptions.csv

# Explicit format overrides even a misleading extension (writes Markdown).
rvsum -D ./exports --format md --summary-file report.txt

# Clean CSV/JSON stdout for scripts.
rvsum -D ./exports -s --format csv > summary.csv
rvsum -D ./exports -s --format json > summary.json
```

CSV/JSON summaries on stdout suppress the trailing human-readable totals. `--print-json` also suppresses them and prints filtered per-VM JSON with capacities in **MiB**. Do not combine stdout-producing flags (`-v`, `--dc-print`, `--plot`, `-p`, etc.) with machine-readable stdout if you need a parseable single document. `--summary-file` leaves the human-readable totals on stdout; file discovery, warnings, exceptions and output-file confirmations for summaries/exceptions go to stderr.

## Filtering and grouping

The name filters take comma-separated **exact, case-sensitive** names, not regular expressions or substrings:

```sh
rvsum -D ./exports --dc-include "Dubai DC",AbuDhabi --cluster-exclude Lab --vm-exclude test-vm -s
rvsum -D ./exports --dc-print
```

Includes narrow the population first; excludes then remove matches. Filters and site maps always use the **real** names from the workbook, also with `--anonymize`; hashing happens afterwards, only on the reported datacenter, cluster and VM names (site-map group names and the `--flatten` labels are not hashed).

### Flatten and flatten-site

- Default: one row per **datacenter / cluster** pair.
- `--flatten-site`: one row per datacenter, with cluster `<DC>_cluster`.
- `--flatten`: one grand-total row labelled `DC1` / `Cluster1`.

Both flatten modes work with every summary format and `--summary-file`. If both flags are supplied together, **neither is applied**. If a site map is supplied, **both are ignored**.

### Site maps

Generate and edit a starter mapping, then use it:

```sh
rvsum --dc-site-map-template  # writes dc_mapper.json in the current directory, then exits
rvsum -D ./exports --dc-site-map dc_mapper.json --summary-file sites.md
```

```json
[
  { "group_name": "Production", "dc_names": ["prod-dc-a", "prod-dc-b"] },
  { "group_name": "DR", "dc_names": ["dr-dc"] }
]
```

Unlike the name filters, mapping uses **case-sensitive substring matching**: each `dc_names` entry matches any datacenter whose name contains it. Unmatched datacenters are **dropped**. Overlapping matches (within one group or across groups) can **count capacity and VMs more than once**; use non-overlapping entries covering every datacenter you intend to include. Each site is labelled `<group_name>_cluster`.

The trailing `Total VMs` / average-size denominator comes from the pre-map VM list, while mapped summary totals come from the mapped rows. For a map that drops or duplicates datacenters, use the structured summary's `totals` with this distinction in mind.

## Podman workflow

For one-off runs, use `podman run --rm` as above. For repeated work, keep a sleeping container with your current directory mounted at `/data`:

```sh
podman pull ghcr.io/shapedthought/rvsum:latest
podman run -d --name rvsum -v "$PWD:/data" -w /data --entrypoint sleep ghcr.io/shapedthought/rvsum:latest infinity
podman exec rvsum rvsum -D . -s
podman exec rvsum rvsum -D . -s --flatten-site --exceptions-file /data/exceptions.csv
```

This intentionally overrides the image's `rvsum` entrypoint with `sleep`; `podman exec` therefore names **both the container and the rvsum executable**. Output files under `/data` appear on the host. Pulling a new image does **not** update an existing container: remove/recreate it to use the new image (your bind-mounted input/output files remain on the host).

The container runs as `appuser`, **UID 1000**. Mounted inputs must be readable by that user; file-output options also need a writable directory. On SELinux hosts, use `-v "$PWD:/data:Z"`. Check host/container ownership when using rootless Podman.

To build a development image from the repository root:

```sh
podman build -t rvsum:dev -f Containerfile .
podman run --rm -v "$PWD:/data" -w /data rvsum:dev -D . -s
# Docker: docker build -t rvsum:dev -f Containerfile .
```

The multi-stage `Containerfile` uses `rust:1-bookworm` and `debian:bookworm-slim`, keeping build/runtime glibc aligned. The default command is `--help`.

## Parsed workbook cache

The first run saves the parsed columns used from **vInfo, vPartition and vDisk**, including both VM UUIDs, to a compact binary sidecar: `exports/.inventory.xlsx.rvsum-cache`. Subsequent runs load these rows instead of reopening the xlsx. Every row is cached, including powered-off VMs; VM selection, disk policies, UUID joins, site maps and anonymization still run afresh, so changing flags does not require a refresh.

- A cache is rebuilt when the input's canonical path, size or modification time (including subsecond precision), the cache schema, or the rvsum version changes. Corrupt caches are silently rebuilt. This uses **size + mtime**, not a full source-content hash: if another program changes the content while preserving both, use `--refresh-cache`.
- If the source directory is not writable (for example, a read-only container mount), rvsum saves under the OS cache directory instead: `$XDG_CACHE_HOME/rvsum` or `~/.cache/rvsum` on Linux, with native cache locations on macOS/Windows. The filename is a hash of the canonical source path. If neither location is writable, the run still succeeds without saving a cache.
- `--no-cache` bypasses **both reads and writes**; `--refresh-cache` rebuilds it. These flags cannot be combined. No cache messages are printed by default; `--verbose` reports hits, misses, refreshes and bypasses (as well as full exceptions).
- Caches contain **real inventory names and UUIDs even with `--anonymize`**. Files are owner-only (`0600`) on Unix, not encrypted. Treat them like the source export. Use `--no-cache` when no extra inventory copy should be written; this does not delete existing caches. They can be removed manually at any time. Unusual unsupported cell types disable saving for that workbook rather than changing validation behaviour.

```bash
rvsum -D ./exports -s                  # Populate/reuse caches automatically
rvsum -D ./exports -s --refresh-cache  # Force a fresh parse
rvsum -D ./exports -s --no-cache       # Do not read or write caches
```

For read-only container inputs, the image's writable home directory provides the fallback. To retain it between disposable containers, mount an appuser-writable cache volume at `/home/appuser/.cache`, or mount a writable directory and set `XDG_CACHE_HOME` to it. A fully read-only container needs such a writable cache mount to save caches.

**Timing:** on the largest checked-in fixture, `sample_rvtools.xlsx` (6,868 bytes), 200 alternating release-binary runs had median whole-command times of **1.04 ms cached vs 1.33 ms with `--no-cache`** (about **1.3×**); forced refresh was 1.46 ms. This fixture is tiny, so process startup dominates; real-estate gains depend on workbook size and storage. These are warm-filesystem timings, not cold disk benchmarks. Only xlsx parsing is skipped: sizing and output are still recalculated.

## Full flag reference

Usage: `rvsum [OPTIONS]`. The following entries reproduce **`rvsum --help` exactly**, grouped by task rather than declaration order. Regression tests check that every flag and its help text stays in sync. `rvsum -h` gives the shorter help form.

<!-- CLI reference start -->
### Input files

```text
  -r, --rvtools-files <RVTOOLS_FILES>...
          RvTools File(s)

  -D, --rvtools-dir <RVTOOLS_DIR>...
          Folder(s) of RvTools files - every .xlsx inside is used

      --recursive
          Descend into subfolders of --rvtools-dir

      --legacy
          Legacy mode - pre v4.1.2

      --no-cache
          Bypass reading and writing the parsed workbook cache

      --refresh-cache
          Rebuild the parsed workbook cache instead of reusing it
```

### VM selection

```text
  -i, --include-powered-off
          Include Powered Off VMs

      --dc-include <DC_INCLUDE>...
          DC include list

      --cluster-include <CLUSTER_INCLUDE>...
          Cluster include list

      --dc-exclude <DC_EXCLUDE>...
          DC exclude list

      --cluster-exclude <CLUSTER_EXCLUDE>...
          Cluster exclude list

      --vm-exclude <VM_EXCLUDE>...
          VM exclude list
```

### Capacity and disk policy

```text
  -d, --do-not-use-vpartition
          Don't use vPartition capacity

      --keep-rdm
          Keep physical-compatibility RDM capacity in the estimate (excluded by default)

      --keep-independent
          Keep independent-mode disk capacity in the estimate (excluded by default)
```

### Grouping and site maps

```text
      --flatten
          Flatten to single site, repo and workload

      --flatten-site
          Flatten to single cluster per-site

      --dc-site-map <DC_SITE_MAP>
          Map DCs to a site - requires a JSON file

      --dc-site-map-template
          Creates Map DC JSON template
```

### Summary, detail and diagnostics

```text
  -s, --show-info
          Print DC level summary

      --format <FORMAT>
          Output format for --show-info / --summary-file / --exceptions-file. If omitted, file outputs infer from the extension (.csv/.json/.md; otherwise table)

          Possible values:
          - table: Box-drawn terminal table (default)
          - json:  JSON object with `rows` and `totals`
          - csv:   CSV with a header row
          - md:    GitHub-flavoured Markdown table with a totals row

      --summary-file <SUMMARY_FILE>
          Write the --show-info summary to a file instead of stdout (implies --show-info)

      --exceptions-file <EXCEPTIONS_FILE>
          Write disk-awareness exceptions to a file (format from --format or file extension)

      --verbose
          Print the full disk-awareness exceptions list to stderr (default is a compact summary). Also report parsed workbook cache activity. Note: short `-v` is already `--vm-table-print`

  -v, --vm-table-print
          Print VM table

      --print-json
          Print the VM info to JSON

      --dc-print
          Print DCs

      --plot
          Plot capacity data in a bar chart

      --anonymize
          Anonymize the data
```

### Legacy VSE export

```text
  -o, --output-file <OUTPUT_FILE>
          Output File [Optional]

  -p, --print
          Print converted data (VSE format)

      --retention <RETENTION>...
          Retention - example 30D1W1M1Y - global
```

### Help and version

```text
  -h, --help
          Print help (see a summary with '-h')

  -V, --version
          Print version
```
<!-- CLI reference end -->

## Legacy VSE export

The old Veeam Size Estimator (VSE) is defunct, but its export remains available for compatibility. It is not the primary purpose of rvsum.

- `-o/--output-file` writes VSE JSON, adding `.json` when the path does not contain it. Unlike summary/exception files, this output is not controlled by `--format`.
- `-p/--print` prints a debug representation of the converted VSE data, **not** a JSON document. Use `-o` for VSE JSON or `--print-json` for per-VM JSON.
- `--retention 30D1W1M1Y` sets the global legacy retention pattern (default 30 days). Legacy workloads use one repository per site, a 24h full / 12h incremental window, and the "Generic Optimistic" data property.

## Known limitations

- **Anonymization scope:** `--anonymize` hashes datacenter, cluster and VM names in the output, not every workbook/exception field (for example disk labels); do not treat it as a complete data-sanitization or security feature.
- **Overlapping exports:** the same VM in two input files is counted twice. It is flagged as `duplicate_vm_name` with `same_uuid_in_multiple_records_check_overlapping_exports` when UUIDs are present. Duplicate *paths* are removed, not duplicate inventory records. Exports without UUID columns join by name, which is ambiguous when names repeat.
- **Guest data quality:** missing/stale Tools or vPartition rows can make the vInfo fallback overestimate guest data. Missing vDisk metadata can prevent exclusions/deduplication. Partial exclusions and shared disks cannot be allocated precisely without usable guest data.
- **Shared disks without UUIDs:** the disk-label-plus-size fallback is ambiguous and can merge unrelated disks. Shared allocation is deterministic by VM name, not by actual ownership or guest usage.
- **Backup coverage:** controller-level SCSI `SharedBus` is not read. In-guest iSCSI/raw volumes appearing only in vPartition are not specially classified. The dedicated `vTools` sheet is not read; only vInfo `Tools` is used. Keeping an RDM/independent disk in the estimate does not make it VADP-backupable.
- **Workbook compatibility:** only the named RVTools `.xlsx` sheets/columns are supported. If a present sheet cannot be found, try opening and re-saving the workbook in Excel with the correct tab name.
- **Site maps:** unmatched datacenters are dropped and overlapping substring matches can double-count. See [site maps](#site-maps).

## Migrating from rvtools2vse

- The crate, library and executable are now **`rvsum`**. Replace `rvtools2vse` in scripts with `rvsum`; flags and output schemas are unchanged. No compatibility binary alias is installed.
- Source links and clones move to [github.com/shapedthought/rvsum](https://github.com/shapedthought/rvsum). After the GitHub repository rename, the old URL redirects. Existing clones can update their remote with `git remote set-url origin https://github.com/shapedthought/rvsum.git`.
- Container images do **not** follow GitHub repository redirects. Switch to **`ghcr.io/shapedthought/rvsum`** and recreate existing containers. The old **`ghcr.io/shapedthought/rvtools2vse` image name is frozen at 1.1.2** (including its old `latest`); it will not receive new releases.
- Releases through 1.1.2 keep their historical `rvtools2vse-*` asset names. New releases use `rvsum-*`; this rename alone does not publish a release or change the package version. A crates.io name being available does not mean the crate has been published there.
- If you installed with Cargo, remove the old binary with `cargo uninstall rvtools2vse`, then install rvsum from source. Check `rvsum --version` and your PATH to confirm which executable is running.

## Development and checks

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo audit
```

CI tests **Rust 1.88 and stable**, checks formatting and Clippy warnings, audits dependencies, and smoke-tests the `rvsum` binary. Regression tests use synthetic workbooks in `testdata/`; regenerate them with `python3 testdata/generate_testdata.py` (requires Python `openpyxl`). Tagged releases build the platform binaries and the multi-architecture GHCR image. See [CI](.github/workflows/ci.yml), [release workflow](.github/workflows/release.yml) and [MIT license](LICENSE).
