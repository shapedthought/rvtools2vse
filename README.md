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
This will make the tool available system wide.
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
  -s, --show-info                             Print DC level summary
      --dc-include <DC_INCLUDE>...            DC include list
      --cluster-include <CLUSTER_INCLUDE>...  Cluster include list
      --dc-exclude <DC_EXCLUDE>...            DC exclude list
      --cluster-exclude <CLUSTER_EXCLUDE>...  Cluster exclude list
      --vm-exclude <VM_EXCLUDE>...            VM exclude list
  -d, --do-not-use-vpartition                 Don't use vPartition capacity
  -v, --vm-table-print                        Print VM table
  -h, --help                                  Print help
  -V, --version                               Print version
```

Note that the vInfo "In Use MiB" must have that string and not "In Use MB", which was used in older RvTools versions.
This also applies to the vPartition "Capacity MiB" ("Capacity MB" in older versions).

## General Flags

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
--show-info
```
This will print a table of the DC-level information, including cluster, capacity and VM count.

This can be useful in deciding if there is anything that needs to be excluded.

This can be run alone with only the RVTools file specified. It will not create a VSE file unless you specify the --output-file flag.

```
-d / --do-not-use-vpartition
```
The tool will also use the vPartition capacity figure if it is available for a VM which normally reduces the capacity.

Using this flag will mean only the vInfo capacity figures will be used.

```
--vm-table-print
```
Prints a table of the VMs and their capacity figures. Useful for checking the VMs that are being included.

## Includes and Excludes

You can use include and exclude items from the results using several flags.

| flag              | Description       |
| ----------------- | ----------------- |
| --dc-include      | Included DCs      |
| --cluster-include | Included Clusters |
| --dc-exclude      | Excluded DCs      |
| --cluster-exclude | Excluded Clusters |
| --vm-exclude      | Excluded VMs      |

The lists that are passed need to be sperated by a comma.

```
rvtools2vse -r rvtools.xlsx --dc-include dc1,dc2
```

If some of the names have a space you will need to pass these as the first items in the list

```
rvtools2vse -r rvtools.xlsx --dc-include "new york dc","france dc",spain_dc
```
## Full Example

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
