mod excel;
mod helpers;
mod models;
mod vse;
use std::{fs, io::Write};

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
