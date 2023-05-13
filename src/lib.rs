mod models;
mod helpers;
mod vse;
use std::{fs, io::Write};

use clap::Parser;
use comfy_table::{Table, presets::UTF8_FULL, modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS}};
use itertools::Itertools;
use office::Excel;
use anyhow::Result;

use crate::{models::{cli::Cli, rvtools::{Vinfo, Vpartition, Datacenter}}, helpers::{get_position, get_string_value, get_float_value}, vse::vse_construct};

pub fn run() -> Result<()> {
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

    // filter out included datacenters and clusters
    if let Some(dc_include) = cli.dc_include {
        info_vec = info_vec
            .into_iter()
            .filter(|x| dc_include.contains(&x.datacenter))
            .collect::<Vec<Vinfo>>();

        println!("Including Datacenters: {:?}", dc_include);
    }

    // filter out included clusters
    if let Some(cluster_include) = cli.cluster_include {
        info_vec = info_vec
            .into_iter()
            .filter(|x| cluster_include.contains(&x.cluster))
            .collect::<Vec<Vinfo>>();

        println!("Including Clusters: {:?}", cluster_include);
    }

    // filter out excluded datacenters
    if let Some(dc_exclude) = cli.dc_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !dc_exclude.contains(&x.datacenter))
            .collect::<Vec<Vinfo>>();

        println!("Excluding Datacenters: {:?}", dc_exclude);
    }

    // filter out excluded clusters
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

    let vse = vse_construct(datacenter_strings, &datacenters)?;

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