# rvtools2vse

Tool to convert RVTools output to VSE format.

Recent updates:

- Updated to work with the VSE v0.11.0 format
- Multiple RVTools files can be passed in
- If vPartition tab is missing it will it will continue to use the vInfo capacity figures only
- Added --dc-site-map-template flag to create a template JSON file for the DC site map
- Added --plot flag to plot the site-level capacity figures in a bar chart
- Added --retention flag to set a custom retention for all workloads
- Added --legacy flag to use the older capacity figures (pre v4.1.2)

## Installation

Requires Rust to be installed

1. clone repo
2. cd into repo
3. run:

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
Usage: rvtools2vse [OPTIONS]

Options:
  -r, --rvtools-files <RVTOOLS_FILES>...      RvTools File(s)
  -i, --include-powered-off                   Include Powered Off VMs
      --retention <RETENTION>...              Retention - example 30D1W1M1Y - global
  -o, --output-file <OUTPUT_FILE>             Output File [Optional]
  -p, --print                                 Print converted data (VSE format)
      --print-json                            Print the VM info to JSON
  -s, --show-info                             Print DC level summary
      --dc-include <DC_INCLUDE>...            DC include list
      --cluster-include <CLUSTER_INCLUDE>...  Cluster include list
      --dc-exclude <DC_EXCLUDE>...            DC exclude list
      --cluster-exclude <CLUSTER_EXCLUDE>...  Cluster exclude list
      --vm-exclude <VM_EXCLUDE>...            VM exclude list
      --legacy                                Legacy mode - pre v4.1.2
      --dc-site-map <DC_SITE_MAP>             Map DCs to a site - requires a JSON file
      --dc-site-map-template                  Creates Map DC JSON template
  -d, --do-not-use-vpartition                 Don't use vPartition capacity
      --dc-print                              Print DCs
  -v, --vm-table-print                        Print VM table
      --flatten                               Flatten to single site, repo and workload
      --flatten-site                          Flatten to single cluster per-site
      --plot                                  Plot capacity data in a bar chart
      --anonymize                             Anonymize the data
  -h, --help                                  Print help
  -V, --version                               Print version
```

If you are using an older RvTools version (pre v4.1.2) you will need to use the --legacy flag. This will used the older "In Use MB"/ "Consumed MB" columns instead of the "In Use MiB"/ "Consumed MiB" columns.

## RvTools columns

The RvTools columns used are:

| Sheet      | Column       |
| ---------- | ------------ |
| vInfo      | VM           |
| vInfo      | powerState   |
| vInfo      | In Use MiB   |
| vInfo      | Datacenter   |
| vInfo      | Cluster      |
| vPartition | VM           |
| vPartition | powerState   |
| vPartition | Consumed MiB |

If any of the vInfo columns are missing or have a different name, the tool will not work. If the vPartition tab is missing or has a different name, the tool will continue to use the vInfo capacity figures only. It will show a warning for the file that is missing that tab at the top of the output.

If any of the Clusters cells are empty they will be shown under an "None" cluster in the results.

## General Flags

Select the file or files to read with the following:

```
-r rvtools1.xlsx

-r rvtools1.xlsx,rvtools2.xlsx
```

Note that the delimiter is a comma.

Also, if there are spaces in the file names, you will need to enclose them in quotes, and pass them at the beginning of the list.

```
-r "rvtools 1.xlsx",rvtools2.xlsx
```

You can modify the output file using different flags (powered off VMs are excluded by default).

```
-i / --include-powered-off
```

In normal operation, the powered-off VMs will be excluded; using this flag will add them to the results.

```
--retention 30D1W1M1Y
```

Custom retention set for all workloads, MUST following the pattern 30D1W1M1Y, even if some of the values are 0.

```
-p / --print
```

Print will display the struct representation of the file to the terminal.

```
--print-json
```

This will print to the screen the current RVTools data in JSON format with all filters applied.

```
-s / --show-info
```

This will print a table of the DC-level information, including cluster, capacity and VM count.

This can be useful in deciding if there is anything that needs to be excluded.

This can be run alone with only the RVTools file specified. It will not create a VSE file unless you specify the --output-file flag.

```
-d / --do-not-use-vpartition
```

The tool will also use the vPartition capacity figure for a VM which normally reduces the capacity in normal use.

Using this flag will mean only the vInfo capacity figures will be used.

```
--dc-print
```

Prints a list of the Datacenters.

```
--vm-table-print
```

Prints a table of the VMs and their capacity figures. Useful for checking the VMs that are being included.

```
--flatten
```

This flag will flatten all the VM counts and capacity into a single Workloaded under a DC called "DC1" and cluster called "Cluster1".

This is useful if you want to quickly aggregate all the results into a single Workload.

```
--flatten-site
```

This flag will flatten the clusters into a single workload per DC (site).

```
--plot
```

This flag will plot the site-level capacity figures in a bar chart (filtering values with less than 1TB of capacity).

It doesn't really help much, but it looks cool, and was fun to write!

```
--anonymize
```

This flag hashes the DC and Cluster information.

```
-o / --output-file vse_rvtools
```

This will create a VSE file with the name specified.

## Includes and Excludes

You can use include and exclude items from the results using several flags.

| flag              | Description       |
| ----------------- | ----------------- |
| --dc-include      | Included DCs      |
| --cluster-include | Included Clusters |
| --dc-exclude      | Excluded DCs      |
| --cluster-exclude | Excluded Clusters |
| --vm-exclude      | Excluded VMs      |

The lists that are passed need to be separated by a comma.

```
rvtools2vse -r rvtools.xlsx --dc-include dc1,dc2
```

If some of the names have a space you will need to pass these as the first items in the list

```
rvtools2vse -r rvtools.xlsx --dc-include "new york dc","france dc",spain_dc
```

## DC Mapping

You can map DC names to a specific site using the --dc_site_map flag passing in the path to a json file with the mapping.

```
--dc_site_map mapping.json
```

The structure of the json file is:

```
[
  {
    "group_name": "DC1",
    "dc_names": ["site1", "site2"]
  },
  {
    "group_name": "DC2",
    "dc_names": ["site3", "site4"]
  }
]
```

```
--dc-site-map-template
```

Creates a template JSON file for the DC site map. Note that this is standalone and the program will exit after creating the file.

You can get the full list of the DC by using the --dc-print flag.

NOTE: There aren't any checks to make sure the DC names are valid, so if you pass in a DC name that doesn't exist it will be ignored.

## Full Examples

```
rvtools2vse -r rvtools.xlsx \
--include-powered-off \
--do-not-use-vpartition \
--dc-exclude dc1 \
--cluster-exclude cluster1,cluster2 \
--vm-exclude vm1,vm2 \
--show-info \
--output-file vse_rvtools.json
```

```
rvtools2vse -r rvtools1.xlsx,rvtools2 \
--dc_site_map mapping.json
--show-info \
--plot \
--output-file vse_rvtools.json
```

## Output file info

- The tool will create a site per Datacenter
- Each Datacenter will have a single performance tier repository
- Each Cluster will be converted into a Workload and assigned to its respective Site (DC) and Repository
- All workloads are assigned the same:
  - 30-day retention period (unless specified using the --retention flag)
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

## Common issues

You may find that the tool cannot find the "vInfo" or "vParition" tabs, to solve this open the file and rename the tabs and save the file.

I do not know why this happens, but I assume that it has something to do with the underlying XML file not being updated with the tab name.
