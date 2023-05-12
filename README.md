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
  -u, --use-vpartition               Use vParition capacity
  -h, --help                         Print help
  -V, --version                      Print version
```
