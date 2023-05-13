use std::{fs, io::Write, path::PathBuf, println};

use anyhow::Result;
use itertools::Itertools;
use office::{DataType, Excel};
mod new_model;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{modifiers::UTF8_SOLID_INNER_BORDERS, Table};
use new_model::{
    CapArchTier, DataProperty, NewVse, PerfTierRepo, Retentions, Site, Window, Workload,
};

use crate::new_model::Backup;

use clap::Parser;
use thiserror::Error;

#[derive(Error, Debug)]
enum MyError {
    #[error("Could not convert enum to string: {0}")]
    EnumToString(String),
    #[error("Could not convert enum to float: {0}")]
    EnumToFloat(String),
    #[error("Could not get position of column: {0}")]
    ColumnPosition(String),
}

#[derive(Parser, Debug)]
#[clap(author, version, about)]
struct Cli {
    /// RVTools File
    #[clap(short, long, value_parser)]
    rvtools_file: PathBuf,

    /// Include Powered Off VMs
    #[clap(short, long, action, default_value_t = false)]
    include_powered_off: bool,

    /// Output File [Optional]
    #[clap(short, long, value_parser)]
    output_file: Option<String>,

    /// Print converted data
    #[clap(short, long, action, default_value_t = false)]
    print: bool,

    /// Print DC level summary
    #[clap(long, action, default_value_t = false)]
    dc_level_info: bool,

    /// DC exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    dc_exclude: Option<Vec<String>>,

    /// Cluster exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    cluster_exclude: Option<Vec<String>>,

    /// VM exclude list
    #[clap(long, value_delimiter = ',', num_args = 1..)]
    vm_exclude: Option<Vec<String>>,

    /// Don't use vPartition capacity
    #[clap(short, long, action, default_value_t = false)]
    do_not_use_vpartition: bool,
}

#[derive(Debug, Clone)]
struct Vinfo {
    vm_name: String,
    datacenter: String,
    cluster: String,
    capacity: f64,
}

#[derive(Debug, Clone)]
struct Vpartition {
    vm_name: String,
    capacity: f64,
}

#[derive(Debug, Clone)]
struct Datacenter {
    name: String,
    cluster: String,
    vm_count: usize,
    capacity: f64,
}

fn get_position(data: &office::Range, col_name: &String) -> Result<usize, MyError> {
    let pos = data.rows()
        .next()
        .unwrap()
        .iter()
        .position(|x| x == &DataType::String(col_name.to_string()));
    
    if let Some(p) = pos {
        Ok(p)
    } else {
        Err(MyError::ColumnPosition(col_name.to_string()))
    }
}

fn get_string_value(data: &DataType, item: String) -> Result<String, MyError> {
    match data {
        DataType::String(t) => Ok(t.to_string()),
        _ => Err(MyError::EnumToString(item)),
    }
}

fn get_float_value(data: &DataType, item: String) -> Result<f64, MyError> {
    match data {
        DataType::Float(t) => Ok(*t),
        DataType::Int(t) => Ok(*t as f64),
        _ => Err(MyError::EnumToFloat(item)),
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let mut excel = Excel::open(cli.rvtools_file).unwrap();

    let workbook = excel.worksheet_range("vInfo").unwrap();

    let vm_column = get_position(&workbook, &"VM".to_string())?;
    let power_column = get_position(&workbook, &"Powerstate".to_string())?;
    let cap_column = get_position(&workbook, &"In Use MiB".to_string())?;
    let dc_column = get_position(&workbook, &"Datacenter".to_string())?;
    let cluster_column = get_position(&workbook, &"Cluster".to_string())?;

    let partition = excel.worksheet_range("vPartition").unwrap();

    let part_vm_column = get_position(&partition, &"VM".to_string())?;
    let part_power_column = get_position(&partition, &"Powerstate".to_string())?;
    let part_cap_column = get_position(&partition, &"Consumed MiB".to_string())?;

    let mut info_vec: Vec<Vinfo> = Vec::new();

    for row in workbook.rows().skip(1) {
        let power_state = get_string_value(&row[power_column], "vInfo - column powerState".to_string())?;
        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }
        let vm_name = get_string_value(&row[vm_column], "vInfo - column 'VM'".to_string())?;
        let cap = get_float_value(&row[cap_column], "vInfo - column 'Capacity MiB'".to_string())?;
        let dc = get_string_value(&row[dc_column], "vInfo - column 'Datacenter'".to_string())?;
        let cluster = get_string_value(&row[cluster_column], "vInfo - column 'Cluster'".to_string())?;

        info_vec.push(Vinfo {
            vm_name: vm_name.to_string(),
            datacenter: dc.to_string(),
            cluster: cluster.to_string(),
            capacity: cap,
        })
    }

    // filter out excluded datacenters and clusters
    if let Some(dc_exclude) = cli.dc_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !dc_exclude.contains(&x.datacenter))
            .collect::<Vec<Vinfo>>();

        println!("Excluding Datacenters: {:?}", dc_exclude);
    }

    if let Some(cluster_exclude) = cli.cluster_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !cluster_exclude.contains(&x.cluster))
            .collect::<Vec<Vinfo>>();

        println!("Excluding Clusters: {:?}", cluster_exclude);
    }

    if let Some(vm_exclude) = cli.vm_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !vm_exclude.contains(&x.vm_name))
            .collect::<Vec<Vinfo>>();

        println!("Excluding VMs: {:?}", vm_exclude);
    }

    let mut part_vec: Vec<Vpartition> = Vec::new();

    for row in partition.rows().skip(1) {
        let power_state = get_string_value(&row[part_power_column], "vParition - column 'powerState'".to_string())?;
        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }
        let vm_name = get_string_value(&row[part_vm_column], "vParition - column 'VM'".to_string())?;
        let cap = get_float_value(&row[part_cap_column], "vParition - column 'Capacity MiB'".to_string())?;

        part_vec.push(Vpartition {
            vm_name: vm_name.to_string(),
            capacity: cap,
        })
    }

    let devisor = 1024_f64.powf(2.0);

    let grouped: Vec<Vpartition> = part_vec
        .into_iter()
        .sorted_by_key(|x| x.vm_name.clone())
        .group_by(|x| x.vm_name.clone())
        .into_iter()
        .map(|(name, group)| {
            let total = group.map(|x| x.capacity).sum();
            Vpartition {
                vm_name: name,
                capacity: total,
            }
        })
        .collect();

    let mut combined: Vec<Vinfo> = Vec::new();

    if !cli.do_not_use_vpartition {
        for i in &info_vec {
            let mut found_match = false;
            for j in &grouped {
                if i.vm_name == j.vm_name {
                    let low_cap = f64::min(i.capacity, j.capacity);
                    let new_st = Vinfo {
                        vm_name: i.vm_name.clone(),
                        datacenter: i.datacenter.clone(),
                        cluster: i.cluster.clone(),
                        capacity: low_cap,
                    };
                    combined.push(new_st);
                    found_match = true;
                }
            }
            if !found_match {
                combined.push(i.clone())
            }
        }
    } else {
        combined = info_vec.clone();
    }

    let mut datacenters: Vec<Datacenter> = Vec::new();

    combined
        .iter()
        .sorted_by_key(|s| (&s.datacenter, &s.cluster))
        .group_by(|s| (&s.datacenter, &s.cluster))
        .into_iter()
        .for_each(|(key, group)| {
            let mut cap = 0.0;
            let mut vm_count = 0;
            group.for_each(|x| {
                cap += x.capacity;
                vm_count += 1;
            });

            datacenters.push(Datacenter {
                name: key.0.to_string(),
                cluster: key.1.to_string(),
                vm_count,
                capacity: cap / devisor,
            })
        });

    if cli.dc_level_info {
        let mut table = Table::new();

        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .apply_modifier(UTF8_SOLID_INNER_BORDERS)
            .set_header(vec!["Datacenter", "Cluster", "Capacity (TB)", "VM Count"]);

        datacenters
            .iter()
            .sorted_by(|a, b| a.capacity.partial_cmp(&b.capacity).unwrap())
            .rev()
            .for_each(|x| {
                table.add_row(vec![
                    x.name.to_string(),
                    x.cluster.to_string(),
                    format!("{:.2}", x.capacity),
                    x.vm_count.to_string(),
                ]);
            });
        println!("{table}");
    }

    let datacenter_strings = datacenters
        .iter()
        .map(|x| format!("{}", x.name))
        .sorted()
        .dedup()
        .collect::<Vec<_>>();

    // construct the new vse file

    // sites
    let sites = datacenter_strings
        .iter()
        .map(|x| Site {
            id: x.to_string(),
            name: x.to_string(),
        })
        .collect::<Vec<Site>>();

    // performance tier repos
    let repos = datacenter_strings
        .iter()
        .map(|x| PerfTierRepo {
            repo_id: format!("{}_repo", x),
            repo_name: format!("{}_repo", x),
            site_id: x.to_string(),
            copy_capacity_tier_enabled: false,
            move_capacity_tier_enabled: false,
            archive_tier_enabled: false,
            capacity_tier_days: 0,
            archive_tier_days: 0,
            capacity_tier_repo_id: "general-s3compatible-capacity".to_string(),
            archive_tier_repo_id: "general-glacier-archive".to_string(),
            storage_type: "xfsRefs".to_string(),
            immutable_cap: false,
            immutable_perf: false,
        })
        .collect::<Vec<PerfTierRepo>>();

    let cap_tier = CapArchTier {
        id: "general-s3compatible-capacity".to_string(),
        tier_type: "Capacity".to_string(),
        name: "General S3 compatible".to_string(),
        default: true,
    };

    let arch_tier = CapArchTier {
        id: "general-glacier-archive".to_string(),
        tier_type: "Archive".to_string(),
        name: "General Amazon S3 Glacier".to_string(),
        default: true,
    };

    let data_property = DataProperty {
        data_property_id: "dpopt".to_string(),
        data_property_name: "Generic Optimistic".to_string(),
        change_rate: 5,
        compression: 50,
        growth_factor: 10,
        default: true,
    };

    let window = Window {
        backup_window_id: "bw12".to_string(),
        backup_window_name: "backup_window1".to_string(),
        full_window: 24,
        incremental_window: 12,
        default: true,
    };

    let retention = Retentions {
        retention_id: "rt1".to_string(),
        retention_name: "30D".to_string(),
        simple: 30,
        weekly: 0,
        monthly: 0,
        yearly: 0,
        default: true,
    };

    let workloads = datacenters
        .iter()
        .map(|x| {
            let backup = Backup {
                retention_id: "rt1".to_string(),
                repo_id: format!("{}_repo", x.name),
                backup_window_id: "bw12".to_string(),
            };

            Workload {
                workload_id: format!("{}_workload", x.cluster),
                enabled: true,
                workload_name: format!("{}_workload", x.cluster),
                site_id: x.name.to_string(),
                large_block: false,
                source_tb: x.capacity,
                units: x.vm_count as i64,
                workload_type: "VM".to_string(),
                data_property_id: "dpopt".to_string(),
                backup,
                copies_enabled: false,
                copies: None,
            }
        })
        .collect::<Vec<Workload>>();

    let vse = NewVse {
        project_length: 3,
        sites,
        repositories: repos,
        cap_arch_tiers: vec![cap_tier, arch_tier],
        data_properties: vec![data_property],
        windows: vec![window],
        retentions: vec![retention],
        workloads,
    };

    if cli.print {
        println!("{:#?}", vse);
    }

    // total vms
    let total_vms = combined.len();
    println!("Total VMs: {}", total_vms);

    let total_cap = datacenters.iter().fold(0.0, |acc, x| acc + x.capacity);

    println!("Total Capacity: {:.2} TB", total_cap);

    if let Some(mut file_name) = cli.output_file {
        if !file_name.contains(".json") {
            file_name.push_str(".json");
        }
        let mut json_file = fs::File::create(&file_name)?;
        let vse_string = serde_json::to_string_pretty(&vse)?;
        json_file.write_all(vse_string.as_bytes())?;

        println!("VSE file written to: {}", file_name);
    }

    Ok(())
}
