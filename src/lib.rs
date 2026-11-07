//! rvsum: offline RVTools summaries for backup sizing.

mod disk;
mod excel;
mod export;
mod helpers;
mod inputs;
mod models;
mod plot;
mod summary;
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
    models::{cli::Cli, new_model::Mapper, rvtools::Datacenter},
    vse::vse_construct,
};
use summary::SummaryFormat;

pub fn run() -> Result<()> {
    let mut cli = Cli::parse();

    if cli.dc_site_map_template {
        let mapper = vec![Mapper {
            group_name: "Group1".to_string(),
            dc_names: vec!["DC1".to_string(), "DC2".to_string()],
        }];

        let mapper_json = serde_json::to_string_pretty(&mapper)?;

        let mut json_file = fs::File::create("dc_mapper.json")?;
        json_file.write_all(mapper_json.as_bytes())?;

        println!("dc_mapper.json created");
        return Ok(());
    }

    cli.rvtools_files =
        inputs::resolve_inputs(&cli.rvtools_files, &cli.rvtools_dir, cli.recursive)?;
    if !cli.rvtools_dir.is_empty() {
        eprintln!("Using {} RVTools file(s):", cli.rvtools_files.len());
        cli.rvtools_files
            .iter()
            .for_each(|f| eprintln!("  {}", f.display()));
    }

    let (info_vec, part_vec, disk_vec) = get_excel(&cli)?;

    let devisor = 1024_f64.powf(2.0);

    let guest_parts = part_vec.clone();
    let grouped: Vec<crate::models::rvtools::Vpartition> = part_vec
        .into_iter()
        .sorted_by_key(|x| x.vm_key.clone())
        .group_by(|x| x.vm_key.clone())
        .into_iter()
        .map(|(key, group)| {
            let mut name = String::new();
            let total = group
                .map(|x| {
                    name.clone_from(&x.vm_name);
                    x.capacity
                })
                .sum();
            crate::models::rvtools::Vpartition {
                vm_name: name,
                vm_key: key,
                capacity: total,
                size: 0.0,
                disk_key: String::new(),
                mount: String::new(),
            }
        })
        .collect();

    let awareness = disk::apply(
        &info_vec,
        &grouped,
        &disk_vec,
        disk::Options {
            use_vpartition: !cli.do_not_use_vpartition,
            keep_rdm: cli.keep_rdm,
            keep_independent: cli.keep_independent,
        },
    );
    let mut awareness = awareness;
    if !disk_vec.is_empty() {
        awareness.exceptions.extend(disk::guest_gap_exceptions(
            &awareness.combined,
            &guest_parts,
            &disk_vec,
            disk::GuestGap {
                percent: cli.guest_gap_threshold,
                min_gib: cli.guest_gap_min_gib,
            },
        ));
    }
    awareness
        .exceptions
        .extend(disk::tools_exceptions(&awareness.combined));
    if let Some(dir) = &cli.export_parquet {
        let (vms, disks) = export::write(
            dir,
            &awareness.combined,
            &disk_vec,
            &export::Options {
                files: &cli.rvtools_files,
                anonymize: cli.anonymize,
                keep_rdm: cli.keep_rdm,
                keep_independent: cli.keep_independent,
            },
        )?;
        eprintln!(
            "Parquet written to: {} and {}",
            vms.display(),
            disks.display()
        );
    }
    if cli.anonymize {
        // Filters and joins above used real names; hash only what is reported.
        awareness
            .exceptions
            .iter_mut()
            .for_each(|e| e.vm_name = anon(&e.vm_name));
    }
    disk::print_exceptions_stderr(&awareness, cli.verbose);
    if !disk_vec.is_empty() {
        eprint!(
            "{}",
            disk::coverage_summary(&awareness.combined, &awareness.excluded)
        );
    }
    let mut combined = awareness.combined;
    let excluded = awareness.excluded;
    if let Some(path) = &cli.exceptions_file {
        let fmt = summary::resolve_format(cli.format, Some(path.as_path()));
        let rendered = disk::render_exceptions(&awareness.exceptions, fmt)?;
        fs::write(path, rendered)?;
        eprintln!("Exceptions written to: {} ({fmt:?})", path.display());
    }

    let mut datacenters: Vec<Datacenter> = Vec::new();

    // Flattens the DC results into single clusters
    if cli.flatten_site && !cli.flatten && cli.dc_site_map.is_none() {
        combined
            .iter()
            .sorted_by_key(|s| &s.datacenter)
            .group_by(|s| &s.datacenter)
            .into_iter()
            .for_each(|(key, group)| {
                let mut cap = 0.0;
                let mut agent = 0.0;
                let mut vm_count = 0;
                group.for_each(|x| {
                    cap += x.capacity;
                    agent += x.non_vadp_capacity;
                    vm_count += 1;
                });

                datacenters.push(Datacenter {
                    name: key.to_string(),
                    cluster: format!("{key}_cluster"),
                    vm_count,
                    capacity: cap / devisor,
                    agent: agent / devisor,
                    excluded: 0.0,
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
                let mut agent = 0.0;
                let mut vm_count = 0;
                group.for_each(|x| {
                    cap += x.capacity;
                    agent += x.non_vadp_capacity;
                    vm_count += 1;
                });

                datacenters.push(Datacenter {
                    name: key.0.to_string(),
                    cluster: key.1.to_string(),
                    vm_count,
                    capacity: cap / devisor,
                    agent: agent / devisor,
                    excluded: 0.0,
                })
            });
    }

    // Excluded non-VADP storage joins its VM's row (a row with no VMs if the
    // whole cluster was excluded), before flatten/site-map aggregation.
    let by_site = cli.flatten_site && !cli.flatten && cli.dc_site_map.is_none();
    for e in &excluded {
        let cluster = if by_site {
            format!("{}_cluster", e.datacenter)
        } else {
            e.cluster.clone()
        };
        let tb = e.capacity_mib / devisor;
        match datacenters
            .iter_mut()
            .find(|d| d.name == e.datacenter && d.cluster == cluster)
        {
            Some(d) => d.excluded += tb,
            None => datacenters.push(Datacenter {
                name: e.datacenter.clone(),
                cluster,
                excluded: tb,
                ..Default::default()
            }),
        }
    }

    if cli.flatten && !cli.flatten_site && cli.dc_site_map.is_none() {
        let vm_count: usize = datacenters.iter().map(|x| x.vm_count).sum();
        let capacity: f64 = datacenters.iter().map(|x| x.capacity).sum();
        let agent: f64 = datacenters.iter().map(|x| x.agent).sum();
        let excluded: f64 = datacenters.iter().map(|x| x.excluded).sum();

        datacenters = vec![];

        datacenters.push(Datacenter {
            name: "DC1".to_string(),
            cluster: "Cluster1".to_string(),
            vm_count,
            capacity,
            agent,
            excluded,
        })
    }

    if let Some(dc_map) = &cli.dc_site_map {
        let mapper_file = fs::read_to_string(dc_map)?;
        let dc_map: Vec<Mapper> = serde_json::from_str(&mapper_file)?;

        let mut temp_dc: Vec<Datacenter> = Vec::new();
        dc_map.iter().for_each(|map_item| {
            let mut cap = 0.0;
            let mut agent = 0.0;
            let mut excluded = 0.0;
            let mut vm_count = 0;

            map_item.dc_names.iter().for_each(|site| {
                for x in datacenters.iter().filter(|x| x.name.contains(site)) {
                    agent += x.agent;
                    excluded += x.excluded;
                }
                let dc_cap: f64 = datacenters
                    .iter()
                    .filter(|x| x.name.contains(site))
                    .map(|x| x.capacity)
                    .sum();
                let dc_vm_count: usize = datacenters
                    .iter()
                    .filter(|x| x.name.contains(site))
                    .map(|x| x.vm_count)
                    .sum();
                cap += dc_cap;
                vm_count += dc_vm_count;
            });

            temp_dc.push(Datacenter {
                name: map_item.group_name.clone(),
                cluster: format!("{}_cluster", map_item.group_name),
                vm_count,
                capacity: cap,
                agent,
                excluded,
            })
        });

        datacenters = temp_dc;
    }

    if cli.anonymize {
        // Site maps and filters matched real names; hash the reported labels now.
        // Site-map group names and the --flatten labels are user/tool labels, not inventory names.
        let flatten_site = cli.flatten_site && !cli.flatten && cli.dc_site_map.is_none();
        let flatten = cli.flatten && !cli.flatten_site && cli.dc_site_map.is_none();
        if cli.dc_site_map.is_none() && !flatten {
            datacenters.iter_mut().for_each(|d| {
                d.name = anon(&d.name);
                d.cluster = if flatten_site {
                    format!("{}_cluster", d.name)
                } else {
                    anon(&d.cluster)
                };
            });
        }
        combined.iter_mut().for_each(|v| {
            v.vm_name = anon(&v.vm_name);
            v.datacenter = anon(&v.datacenter);
            v.cluster = anon(&v.cluster);
        });
    }

    let show_info = cli.show_info || cli.summary_file.is_some();
    let summary_fmt = summary::resolve_format(cli.format, cli.summary_file.as_deref());
    // json/csv to stdout must stay pipeable: suppress the trailing totals lines
    let machine_stdout = cli.show_info
        && cli.summary_file.is_none()
        && matches!(summary_fmt, SummaryFormat::Json | SummaryFormat::Csv);

    if show_info {
        let rendered = summary::render(&datacenters, summary_fmt)?;
        match &cli.summary_file {
            Some(path) => {
                fs::write(path, rendered)?;
                eprintln!("Summary written to: {} ({summary_fmt:?})", path.display());
            }
            None => print!("{rendered}"),
        }
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
                    x.vpartition_capacity
                        .map(|v| format!("{:.2}", v / gb_devisor))
                        .unwrap_or_else(|| "-".to_string()),
                    x.powerstate.to_string(),
                ]);
            });
        println!("{table}");
    }

    let datacenter_strings = datacenters
        .iter()
        .map(|x| x.name.to_string())
        .sorted()
        .dedup()
        .collect::<Vec<_>>();

    if cli.dc_print {
        datacenter_strings.iter().for_each(|x| println!("{x:?},"))
    }

    let vse = vse_construct(datacenter_strings, &datacenters, cli.retention)?;

    if cli.print {
        println!("{vse:#?}");
    }

    if cli.print_json {
        let combined_json = serde_json::to_string_pretty(&combined)?;
        println!("{combined_json}");
    }

    if cli.plot {
        let data = datacenters
            .iter()
            .filter(|x| x.capacity > 1.00)
            .map(|x| x.capacity)
            .collect::<Vec<_>>();
        let site_names = datacenters
            .iter()
            .filter(|x| x.capacity > 1.00)
            .map(|x| x.name.clone())
            .collect::<Vec<_>>();
        plot::plot_data(data, site_names);
    }

    if !cli.print_json && !machine_stdout {
        let total_vms = combined.len();
        println!("Total VMs: {total_vms}");

        let total_cap = datacenters.iter().fold(0.0, |acc, x| acc + x.capacity);

        println!("Total Capacity: {total_cap:.2} TB");

        let average_vm = (total_cap * 1024.0) / total_vms as f64;

        println!("Average VM Size: {average_vm:.2} GB");
    }

    if let Some(mut file_name) = cli.output_file {
        if !file_name.contains(".json") {
            file_name.push_str(".json");
        }
        let mut json_file = fs::File::create(&file_name)?;
        let vse_string = serde_json::to_string_pretty(&vse)?;
        json_file.write_all(vse_string.as_bytes())?;

        println!("VSE file written to: {file_name}");
    }

    Ok(())
}

pub(crate) fn anon(item: &str) -> String {
    fastmurmur3::hash(item.as_bytes()).to_string()
}
