# rvtools2vse

Tool to convert RVTools output to VSE format

## Installation

Requires Rust to be installed

1. Clone repo
2. cd into repo
3. Run:

```
cargo install --path .
```

## Uninstall

```
cargo uninstall rvtools2vse
```

## Usage

```
Usage: rvtools2vse.exe [OPTIONS] --rvtools-file <RVTOOLS_FILE> --output-file <OUTPUT_FILE>

Options:
  -r, --rvtools-file <RVTOOLS_FILE>  RVTools File
  -i, --include-powered-off          Include Powered Off VMs
  -o, --output-file <OUTPUT_FILE>    Output File
  -p, --print                        Print converted data
  -d, --do-not-use-vpartition        Don't use vPartition capacity
  -h, --help                         Print help
  -V, --version                      Print version
```

Note that the vInfo In Use MiB must be have that string and not In Use MB which was used in older RvTools versions.
This also applies to the vPartition Capacity MiB (Capacity MB in older versions).

The tool in normal operation will filter out powered off VMs. If you want to include them, use the `--include-powered-off` flag.

The tool will also use the vPartition capacity figure if it is available for a VM which normally reduces the capacity. If you don't want to use this, use the `--do-not-use-vpartition` flag.
