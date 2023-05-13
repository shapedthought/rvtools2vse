# rvtools2vse

Tool to convert RVTools output to VSE format

## Installation

Requires Rust to be installed

1. clone repo
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
Usage: rvtools2vse [OPTIONS] --rvtools-file <RVTOOLS_FILE>

Options:
  -r, --rvtools-file <RVTOOLS_FILE>           RVTools File
  -i, --include-powered-off                   Include Powered Off VMs
  -o, --output-file <OUTPUT_FILE>             Output File [Optional]
  -p, --print                                 Print converted data
      --dc-level-info                         Print DC-level summary
      --dc-exclude <DC_EXCLUDE>...            DC exclude list
      --cluster-exclude <CLUSTER_EXCLUDE>...  Cluster exclude list
      --vm-exclude <VM_EXCLUDE>...            VM exclude list
  -d, --do-not-use-vpartition                 Don't use vPartition capacity
  -h, --help                                  Print help
  -V, --version                               Print version
```

Note that the vInfo "In Use MiB" must have that string and not In Use MB, which was used in older RvTools versions.
This also applies to the vPartition "Capacity MiB" (Capacity MB in older versions).

## Flags

You can modify the output file using different flags.

```
-i / --include-powered-off
```

In normal operation, the powered-off VMs will be excluded; using this flag will add them to the results.

```
-p / --print
```

Print will display the struct representation of the file to the terminal.

```
--dc-level-info
```

This will print a table of the DC-level information, including cluster, capacity and VM count.

This can be useful in deciding if there is anything that needs to be excluded.

This can be run alone with only the RVTools file specified. It will not create a VSE file unless you specify the --output-file flag.

```
--dc-exclude dc1,dc2
```

You can pass a list of DC names to this flag, and they will be filtered out of the results.

```
--cluster-exclude cluster1,cluster2
```

Like with DC exclude, you can also pass a list of clusters to exclude.

```
--vm-exclude vm1,vm2
```

This is the same as DC and Cluster exclude.

```
-d / --do-not-use-vpartition
```

The tool will also use the vPartition capacity figure if it is available for a VM which normally reduces the capacity.

Using this flag will mean only the vInfo capacity figures will be used.

### Full Example

```
rvtools2vse -r rvtools.xlsx \
--include-powered-off \
--do-not-use-vpartition  \
--dc-exclude dc1 \
--cluster-exclude cluster1,cluster2 \
--vm-exclude vm1,vm2 \
--print \
--output-file vse_rvtools.json
```

## Output file info

- The tool will create a site per Datacenter
- Each Datacenter will have a single performance tier repository
- Each Cluster will be converted into a Workload and assigned to its respective Site (DC) and Repository
- All workloads are assigned the same:
  - 30-day retention period
  - 24 full/ 12 inc hour backup window
  - "Generic Optimistic" data property
- Repositories are set to use ReFS/XFS

The aim is to get the data into the VSE, and which point you can modify it as required.

## vPartition capacity

The tool in normal use will read the vPartition tab, group all the partitions for a VM together, and create a total VM capacity figure.

```
VM1 100GB
  Partition1 50GB
  Partition2 50GB
```

The tool then goes through all the vInfo VMs, and where there is a match on the VM name and the vParition value is lower than the vInfo value, the vParition value is used.
