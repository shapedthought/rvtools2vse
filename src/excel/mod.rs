use crate::{helpers, models::errors::MyError};
use calamine::{open_workbook, Reader, Xlsx, XlsxError};
use helpers::{as_display_string, is_physical_rdm_mode, ColPosition, GetFloat, GetString};
use fastmurmur3;

use crate::models::{
    cli::Cli,
    rvtools::{Vdisk, Vinfo, Vpartition},
};

type ExcelSheets = (Vec<Vinfo>, Vec<Vpartition>, Vec<Vdisk>);

pub fn get_excel(cli: &Cli) -> Result<ExcelSheets, MyError> {
    let mut excel_vec: Vec<Xlsx<_>> = Vec::new();

    if cli.rvtools_files.len() == 0 {
        return Err(MyError::RvtoolsError(
            "No RVTools file or files specified".to_string(),
        ));
    }

    for file in &cli.rvtools_files {
        let new_excel: Xlsx<_> = open_workbook(file)?;
        excel_vec.push(new_excel);
    }

    let mut info_vec: Vec<Vinfo> = Vec::new();
    let mut part_vec: Vec<Vpartition> = Vec::new();
    let mut disk_vec: Vec<Vdisk> = Vec::new();

    for (i, mut excel) in excel_vec.into_iter().enumerate() {
        let workbook = match excel.worksheet_range("vInfo") {
            Ok(range) => range,
            Err(XlsxError::WorksheetNotFound(_)) => {
                return Err(MyError::VinfoError("vInfo sheet not found".to_string()));
            }
            Err(e) => return Err(e.into()),
        };

        let vm_column = workbook.get_col_pos(&"VM".to_string())?;

        let power_column = workbook.get_col_pos(&"Powerstate".to_string())?;

        let cap_string = if cli.legacy {
            "In Use MB"
        } else {
            "In Use MiB"
        };

        let cap_column = workbook.get_col_pos(&cap_string.to_string())?;

        let dc_column = workbook.get_col_pos(&"Datacenter".to_string())?;

        let cluster_column = workbook.get_col_pos(&"Cluster".to_string())?;

        for row in workbook.rows().enumerate().skip(1) {
            let power_state = &row.1[power_column]
                .get_string_value("vInfo - column Powerstate vInfo".to_string(), row.0 + 1)?;

            if power_state.contains("poweredOff") && !cli.include_powered_off {
                continue;
            }

            let mut vm_name =
                &row.1[vm_column].get_string_value("vInfo - column 'VM'".to_string(), row.0 + 1)?;

            let anonymized_vm_name = anon_data(&vm_name, &cli);
            vm_name = &anonymized_vm_name;

            let cap_error_string = if cli.legacy {
                "vInfo - column 'Capacity MB'"
            } else {
                "vInfo - column 'Capacity MiB'"
            };

            let cap = &row.1[cap_column]
                .get_float_value(cap_error_string.to_string(), row.0 + 1)?;

            let mut dc = &row.1[dc_column]
                .get_string_value("vInfo - column 'Datacenter'".to_string(), row.0 + 1)?;

            let mut anan_dc = dc.clone();
            if cli.anonymize {
                let hash = fastmurmur3::hash(dc.as_bytes());
                anan_dc = hash.to_string();
            };
            dc = &anan_dc;

            let mut cluster = &row.1[cluster_column]
                .get_string_value("vInfo - column 'Cluster'".to_string(), row.0 + 1)?;

            let mut anon_cluster = cluster.clone();
            if cli.anonymize { 
                let hash = fastmurmur3::hash(cluster.as_bytes());
                anon_cluster = hash.to_string();
            }
            cluster = &anon_cluster;

            info_vec.push(Vinfo {
                vm_name: vm_name.to_string(),
                datacenter: dc.to_string(),
                cluster: cluster.to_string(),
                capacity: *cap,
                powerstate: power_state.to_string(),
                vpartition_capacity: None,
            })
        }
        if let Some(dc_include) = &cli.dc_include {
            info_vec = info_vec
                .into_iter()
                .filter(|x| dc_include.contains(&x.datacenter))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(cluster_include) = &cli.cluster_include {
            info_vec = info_vec
                .into_iter()
                .filter(|x| cluster_include.contains(&x.cluster))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(dc_exclude) = &cli.dc_exclude {
            info_vec = info_vec
                .into_iter()
                .filter(|x| !dc_exclude.contains(&x.datacenter))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(cluster_exclude) = &cli.cluster_exclude {
            info_vec = info_vec
                .into_iter()
                .filter(|x| !cluster_exclude.contains(&x.cluster))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(vm_exclude) = &cli.vm_exclude {
            info_vec = info_vec
                .into_iter()
                .filter(|x| !vm_exclude.contains(&x.vm_name))
                .collect::<Vec<Vinfo>>();
        }

        // calamine >= 0.20 returns Err(WorksheetNotFound) instead of None for a
        // missing sheet; map it back to Option<Result<_>> to keep behaviour.
        let partition = match excel.worksheet_range("vPartition") {
            Err(XlsxError::WorksheetNotFound(_)) => None,
            other => Some(other),
        };

        if let Some(partition) = partition {
            if partition.is_err() || cli.do_not_use_vpartition {
                if !cli.do_not_use_vpartition {
                    Err(MyError::VpartitionError(
                        "vPartition sheet not found".to_string(),
                    ))?;
                }
            } else {
                let partition = partition.unwrap();

                let part_vm_column = partition.get_col_pos(&"VM".to_string())?;

                // Powerstate is optional on vPartition: power state is already
                // filtered on vInfo, and partitions only count when they join to
                // a vInfo VM, so a missing column just skips the extra filter.
                let part_power_column = partition.get_col_pos(&"Powerstate".to_string()).ok();
                if part_power_column.is_none() {
                    eprintln!(
                        "vPartition in {:?} has no Powerstate column; using vInfo power state via the VM join.",
                        cli.rvtools_files[i]
                    );
                }

                let consumed_string = if cli.legacy {
                    "Consumed MB"
                } else {
                    "Consumed MiB"
                };

                let part_cap_column = partition.get_col_pos(&consumed_string.to_string())?;
                for row in partition.rows().enumerate().skip(1) {
                    if let Some(col) = part_power_column {
                        let power_state = &row.1[col].get_string_value(
                            "vParition - column 'Powerstate'".to_string(),
                            row.0 + 1,
                        )?;

                        if power_state.contains("poweredOff") && !cli.include_powered_off {
                            continue;
                        }
                    }

                    let vm_name = &row.1[part_vm_column]
                        .get_string_value("vParition - column 'VM'".to_string(), row.0 + 1)?;

                    let cap_error_string = if cli.legacy {
                        "vParition - column 'Capacity MB'"
                    } else {
                        "vParition - column 'Capacity MiB'"
                    };

                    let cap = &row.1[part_cap_column].get_float_value(
                        cap_error_string.to_string(),
                        row.0 + 1,
                    )?;

                    // Hash with the same mapping as vInfo so the join still matches
                    part_vec.push(Vpartition {
                        vm_name: anon_data(vm_name, cli),
                        capacity: *cap,
                    })
                }
            }
        } else {
            if !cli.do_not_use_vpartition {
                eprintln!(
                    "vPartition sheet not found in {:?}, continuing without it.",
                    cli.rvtools_files[i]
                );
            }
        }

        let disk_sheet = match excel.worksheet_range("vDisk") {
            Err(XlsxError::WorksheetNotFound(_)) => None,
            other => Some(other),
        };

        match disk_sheet {
            None => {
                eprintln!(
                    "vDisk sheet not found in {:?}, continuing without disk awareness.",
                    cli.rvtools_files[i]
                );
            }
            Some(Err(e)) => return Err(e.into()),
            Some(Ok(disk)) => {
                let disk_vm_column = disk.get_col_pos(&"VM".to_string())?;
                let disk_cap_string = if cli.legacy {
                    "Capacity MB"
                } else {
                    "Capacity MiB"
                };
                let disk_cap_column = disk.get_col_pos(&disk_cap_string.to_string())?;
                let disk_mode_column = disk.get_col_pos(&"Raw Comp. Mode".to_string()).ok();
                if disk_mode_column.is_none() {
                    eprintln!(
                        "vDisk in {:?} has no 'Raw Comp. Mode' column; physical RDMs cannot be detected.",
                        cli.rvtools_files[i]
                    );
                }
                let disk_label_column = disk.get_col_pos(&"Disk".to_string()).ok();
                let disk_power_column = disk.get_col_pos(&"Powerstate".to_string()).ok();

                for row in disk.rows().enumerate().skip(1) {
                    if let Some(col) = disk_power_column {
                        let power_state = as_display_string(&row.1[col]);
                        if power_state.contains("poweredOff") && !cli.include_powered_off {
                            continue;
                        }
                    }

                    let vm_name = &row.1[disk_vm_column]
                        .get_string_value("vDisk - column 'VM'".to_string(), row.0 + 1)?;

                    let cap = &row.1[disk_cap_column].get_float_value(
                        format!("vDisk - column '{disk_cap_string}'"),
                        row.0 + 1,
                    )?;

                    let raw_comp = disk_mode_column
                        .map(|c| as_display_string(&row.1[c]))
                        .unwrap_or_default();
                    // get_string_value maps Empty -> "None"; treat that as blank.
                    let raw_comp = if raw_comp == "None" {
                        String::new()
                    } else {
                        raw_comp
                    };

                    let label = disk_label_column
                        .map(|c| as_display_string(&row.1[c]))
                        .filter(|s| !s.is_empty() && s != "None")
                        .unwrap_or_else(|| format!("disk@row{}", row.0 + 1));

                    disk_vec.push(Vdisk {
                        vm_name: anon_data(vm_name, cli),
                        capacity: *cap,
                        disk_label: label,
                        raw_comp_mode: raw_comp.clone(),
                        is_physical_rdm: is_physical_rdm_mode(&raw_comp),
                    });
                }
            }
        }
    }

    Ok((info_vec, part_vec, disk_vec))
}

fn anon_data(item: &String, cli: &Cli) -> String {
    let mut anon_item = item.clone();
    if cli.anonymize {
        let hash = fastmurmur3::hash(anon_item.as_bytes());
        anon_item = hash.to_string();
    }
    anon_item
}
