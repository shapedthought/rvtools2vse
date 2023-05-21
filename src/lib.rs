mod excel;
mod helpers;
mod models;
mod vse;
use std::{fs, io::Write, println};

use anyhow::Result;
use clap::Parser;
use comfy_table::{
    modifiers::{UTF8_ROUND_CORNERS, UTF8_SOLID_INNER_BORDERS},
    presets::UTF8_FULL,
    Table,
};
use itertools::Itertools;

use crate::{
    excel::get_excel,
    models::{
        cli::Cli,
        rvtools::{Datacenter, Vinfo, Vpartition},
    },
    vse::vse_construct,
};

pub fn run() -> Result<()> {
    let cli = Cli::parse();

    let (info_vec, part_vec) = get_excel(&cli)?;

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
                        powerstate: i.powerstate.clone(),
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

    // Flattens the DC results into single clusters
    if cli.flatten_site && !cli.flatten {
        combined
            .iter()
            .sorted_by_key(|s| &s.datacenter)
            .group_by(|s| &s.datacenter)
            .into_iter()
            .for_each(|(key, group)| {
                let mut cap = 0.0;
                let mut vm_count = 0;
                group.for_each(|x| {
                    cap += x.capacity;
                    vm_count += 1;
                });

                datacenters.push(Datacenter {
                    name: key.to_string(),
                    cluster: format!("{}_cluster", key.to_string()),
                    vm_count,
                    capacity: cap / devisor,
                })
            });
    } else {
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
    }

    if cli.flatten && !cli.flatten_site {
        let vm_count: usize = datacenters.iter().map(|x| x.vm_count).sum();
        let capacity: f64 = datacenters.iter().map(|x| x.capacity).sum();

        datacenters = vec![];

        datacenters.push(Datacenter {
            name: "DC1".to_string(),
            cluster: "Cluster1".to_string(),
            vm_count,
            capacity,
        })
    }

    if cli.show_info {
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

    if cli.vm_table_print {
        let mut table = Table::new();

        table
            .load_preset(UTF8_FULL)
            .apply_modifier(UTF8_ROUND_CORNERS)
            .apply_modifier(UTF8_SOLID_INNER_BORDERS)
            .set_header(vec![
                "Datacenter",
                "Cluster",
                "VM Name",
                "Capacity (GiB)",
                "vPartition (GiB)",
                "Power State",
            ]);

        let gb_devisor = 1024_f64.powf(1.0);

        combined
            .iter()
            .sorted_by(|a, b| a.capacity.partial_cmp(&b.capacity).unwrap())
            .rev()
            .for_each(|x| {
                table.add_row(vec![
                    x.datacenter.to_string(),
                    x.cluster.to_string(),
                    x.vm_name.to_string(),
                    format!("{:.2}", x.capacity / gb_devisor),
                    format!("{:.2}", x.capacity / gb_devisor),
                    x.powerstate.to_string(),
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
    
    if cli.dc_print {
        datacenter_strings
            .iter()
            .for_each(|x| println!("{:?}", x))
    }

    let vse = vse_construct(datacenter_strings, &datacenters)?;

    if cli.print {
        println!("{:#?}", vse);
    }

    // total vms
    let total_vms = combined.len();
    println!("Total VMs: {}", total_vms);

    let total_cap = datacenters.iter().fold(0.0, |acc, x| acc + x.capacity);

    println!("Total Capacity: {:.2} TB", total_cap);

    let average_vm = (total_cap * 1024.0) / total_vms as f64;

    println!("Average VM Size: {:.2} GB", average_vm);

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
