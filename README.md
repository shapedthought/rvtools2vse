# rvtools2vse

A fast, offline command-line summariser for [RVTools](https://www.dell.com/support/kbdoc/en-gb/000325532/rvtools-4-7-1-installer) `.xlsx` exports. RVTools is now owned by Dell (previously Robware).

It reads the `vInfo` (and, if present, `vPartition`) tabs from one or more RVTools files, filters VMs, and rolls up VM count and capacity per datacenter and cluster. You can print the summary as a terminal table, JSON, CSV or Markdown, or write it to a file.

> **Legacy VSE output.** The tool was originally written to convert RVTools data into the Veeam Size Estimator (VSE) format. VSE is now defunct. The VSE JSON output is still available via `-o/--output-file` and `-p/--print` but is no longer the main purpose of the tool.

## Building from source

Requires Rust **1.88 or newer** (`rust-version` in `Cargo.toml`).

```
git clone https://github.com/shapedthought/rvtools2vse.git
cd rvtools2vse
cargo install --path .     # installs rvtools2vse on your PATH
# or: cargo build --release  -> target/release/rvtools2vse
```

Uninstall with `cargo uninstall rvtools2vse`.

## Container

A multi-stage `Containerfile` is included (`rust:1-bookworm` builder, `debian:bookworm-slim` runtime). Build it from the repo root with podman or docker:

```
podman build -t rvtools2vse:latest -f Containerfile .
# or: docker build -t rvtools2vse:latest -f Containerfile .
```

Mount the folder that holds your RVTools files at `/data` (the container's working directory):

```
podman run --rm -v "$PWD":/data rvtools2vse:latest -D /data -s --format md
podman run --rm -v "$PWD":/data rvtools2vse:latest -r /data/rvtools.xlsx -s --format csv > summary.csv
```

The container runs as **uid 1000** (`appuser`). Reading files only needs read access. Writing files (`--summary-file`, `-o`, `--dc-site-map-template`) needs a mount that uid 1000 can write to. On SELinux hosts, add `:Z` to the volume (`-v "$PWD":/data:Z`). With rootless podman, `--userns=keep-id` maps uid 1000 to your own user.

## Usage

Output of `rvtools2vse -h` (use `--help` for the long form):

```
Usage: rvtools2vse [OPTIONS]

Options:
  -r, --rvtools-files <RVTOOLS_FILES>...
          RvTools File(s)
  -D, --rvtools-dir <RVTOOLS_DIR>...
          Folder(s) of RvTools files - every .xlsx inside is used
      --recursive
          Descend into subfolders of --rvtools-dir
  -i, --include-powered-off
          Include Powered Off VMs
      --retention <RETENTION>...
          Retention - example 30D1W1M1Y - global
  -o, --output-file <OUTPUT_FILE>
          Output File [Optional]
  -p, --print
          Print converted data (VSE format)
      --print-json
          Print the VM info to JSON
  -s, --show-info
          Print DC level summary
      --format <FORMAT>
          Output format for the --show-info summary [default: table] [possible values: table, json, csv, md]
      --summary-file <SUMMARY_FILE>
          Write the --show-info summary to a file instead of stdout (implies --show-info)
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
      --legacy
          Legacy mode - pre v4.1.2
      --dc-site-map <DC_SITE_MAP>
          Map DCs to a site - requires a JSON file
      --dc-site-map-template
          Creates Map DC JSON template
  -d, --do-not-use-vpartition
          Don't use vPartition capacity
      --dc-print
          Print DCs
  -v, --vm-table-print
          Print VM table
      --flatten
          Flatten to single site, repo and workload
      --flatten-site
          Flatten to single cluster per-site
      --plot
          Plot capacity data in a bar chart
      --anonymize
          Anonymize the data
  -h, --help
          Print help (see more with '--help')
  -V, --version
          Print version
```

### Input files

```
-r rvtools1.xlsx,rvtools2.xlsx        # comma-separated; quote names with spaces
-D ./exports                          # every .xlsx in a folder
-D ./site_a,./site_b --recursive      # several folders, including subfolders
```

- `-r` and `-D` can be combined. Any file picked up twice is only read once. At least one input is required.
- `-D` matches `.xlsx` regardless of case, skips hidden files and Excel `~$` lock files, and processes files in sorted order. `--recursive` also searches subfolders, skipping hidden ones. An empty or missing folder is an error.
- Use `--legacy` for RVTools exports older than v4.1.2. It reads the `In Use MB` / `Consumed MB` columns instead of the `MiB` ones.

Columns read (names must match exactly):

| Sheet      | Columns |
| ---------- | ------- |
| vInfo      | `VM`, `Powerstate`, `Datacenter`, `Cluster`, `In Use MiB` |
| vPartition | `VM`, `Consumed MiB`, `Powerstate` (optional) |

If `vPartition` has no `Powerstate` column, a note is printed to stderr and power state comes from the matching vInfo row (partitions only count when they join to an included vInfo VM). If a workbook has no `vPartition` tab, a warning is printed and vInfo capacity is used for that file. VMs with an empty cluster are grouped under `None`.

### Capacity

By default, the tool sums each VM's vPartition `Consumed` values and uses the lower of that total and the vInfo `In Use` value. `-d/--do-not-use-vpartition` uses vInfo only. Powered-off VMs are excluded unless you pass `-i/--include-powered-off`.

### Filtering

`--dc-include`, `--cluster-include`, `--dc-exclude`, `--cluster-exclude` and `--vm-exclude` each take a comma-separated list. Put any quoted names containing spaces first, e.g. `--dc-include "new york dc",spain_dc`. `--dc-print` lists the datacenter names it found.

### Summary output (`-s/--show-info`)

| `--format` | Output |
| ---------- | ------ |
| `table`    | Default. Box-drawn terminal table. |
| `json`     | `{"rows": [{"datacenter","cluster","capacity_tb","vm_count"}], "totals": {"capacity_tb","vm_count"}}`. Capacity is unrounded. |
| `csv`      | Header `datacenter,cluster,capacity_tb,vm_count`, then one row per cluster. Capacity to 2 decimals. |
| `md`       | GitHub Markdown table with a final **Total** row. |

Rows are sorted by capacity, largest first. Capacity is in TB (binary, base 1024).

`--summary-file <path>` writes the rendered summary to that file instead of stdout, and implies `-s`. The path is used **exactly as given**, with no extension added, so match it to the format (`--format csv --summary-file summary.csv`).

### Grouping: `--flatten`, `--flatten-site`, `--dc-site-map`

- `--flatten-site` gives one row per datacenter, with the cluster shown as `<DC>_cluster`.
- `--flatten` combines everything into a single `DC1` / `Cluster1` row.
- `--dc-site-map map.json` groups datacenters into named sites. A datacenter belongs to a group when its name *contains* one of the group's `dc_names`; datacenters that match nothing are dropped. Run `--dc-site-map-template` to write a starter `dc_mapper.json` (the tool exits after creating it).

```
[
  { "group_name": "Site1", "dc_names": ["dc-a", "dc-b"] },
  { "group_name": "Site2", "dc_names": ["dc-c"] }
]
```

When `--dc-site-map` is given, `--flatten` and `--flatten-site` are ignored. If `--flatten` and `--flatten-site` are passed together, neither is applied.

### stdout vs stderr

- The summary (unless `--summary-file` is set), `--print-json`, `-v`, `-p`, `--dc-print` and `--plot` go to **stdout**.
- The trailing `Total VMs / Total Capacity / Average VM Size` lines also go to stdout. They are left out when `--print-json` is set, or when `--format json|csv` is written to stdout, so that output can be piped.
- The list of files picked up by `-D`, missing-vPartition warnings and the "Summary written to" message go to **stderr**.

### Other options

- `--print-json` prints the filtered per-VM data as JSON.
- `-v/--vm-table-print` prints a per-VM table. `Capacity (GiB)` is the capacity used in the totals, and `vPartition (GiB)` is the summed vPartition value for that VM (`-` if it has no vPartition rows).
- `--plot` draws a terminal bar chart of per-site capacity (sites over 1 TB).
- `--anonymize` hashes DC, cluster and VM names. VM names are hashed the same way in vInfo and vPartition, so totals match a non-anonymized run.
- `-o/--output-file`, `-p/--print` and `--retention` are for the legacy VSE output. `-o` appends `.json` if it is missing. Each datacenter becomes a site with one repository, and each cluster becomes a workload. Workloads get 30-day retention (or the `--retention 30D1W1M1Y` pattern you give), a 24h full / 12h incremental window and the "Generic Optimistic" data property.

## Examples

```
# Quick summary of every export in a folder
rvtools2vse -D ./exports -s

# One row per site as Markdown, written to a file
rvtools2vse -D ./exports --flatten-site --format md --summary-file summary.md

# CSV for a spreadsheet, including powered-off VMs, excluding a cluster
rvtools2vse -r rv1.xlsx,rv2.xlsx -i --cluster-exclude test-cl -s --format csv > summary.csv

# Grand total as JSON
rvtools2vse -D ./exports --flatten -s --format json | jq '.totals'

# Group DCs into sites
rvtools2vse --dc-site-map-template          # edit dc_mapper.json
rvtools2vse -D ./exports --dc-site-map dc_mapper.json -s --format md
```

## Testing

`cargo test` runs the unit tests and the regression tests in `tests/regression.rs`. Those tests use the synthetic workbooks in `testdata/`; regenerate them with `python3 testdata/generate_testdata.py` (needs `openpyxl`).

## Known issues

- If the tool can't find the `vInfo` or `vPartition` tab even though it exists, open the file in Excel, rename the tab, and save it again.
- With `--anonymize`, the include/exclude filters are compared against the hashed names, so filtering by real DC, cluster or VM names has no effect.
