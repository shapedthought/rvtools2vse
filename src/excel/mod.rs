use crate::{helpers, models::errors::MyError};
use calamine::{open_workbook, Reader, Xlsx};
use helpers::{ColPosition, GetFloat, GetString};

use crate::models::{
    cli::Cli,
    rvtools::{Vinfo, Vpartition},
};

pub fn get_excel(cli: &Cli) -> Result<(Vec<Vinfo>, Vec<Vpartition>), MyError> {
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

    for (i, mut excel) in excel_vec.into_iter().enumerate() {
        let workbook = excel.worksheet_range("vInfo");

        if workbook.is_none() {
            return Err(MyError::VinfoError("vInfo sheet not found".to_string()));
        }

        let workbook = workbook.unwrap().unwrap();

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

            let vm_name =
                &row.1[vm_column].get_string_value("vInfo - column 'VM'".to_string(), row.0 + 1)?;

            let cap_error_string = if cli.legacy {
                "vInfo - column 'Capacity MB'"
            } else {
                "vInfo - column 'Capacity MiB'"
            };

            let cap = &row.1[cap_column]
                .get_float_value(cap_error_string.to_string(), row.0 + 1)?;

            let dc = &row.1[dc_column]
                .get_string_value("vInfo - column 'Datacenter'".to_string(), row.0 + 1)?;

            let cluster = &row.1[cluster_column]
                .get_string_value("vInfo - column 'Cluster'".to_string(), row.0 + 1)?;

            info_vec.push(Vinfo {
                vm_name: vm_name.to_string(),
                datacenter: dc.to_string(),
                cluster: cluster.to_string(),
                capacity: *cap,
                powerstate: power_state.to_string(),
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

        let partition = excel.worksheet_range("vPartition");

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

                let part_power_column = partition.get_col_pos(&"Powerstate".to_string())?;

                let consumed_string = if cli.legacy {
                    "Consumed MB"
                } else {
                    "Consumed MiB"
                };

                let part_cap_column = partition.get_col_pos(&consumed_string.to_string())?;
                for row in partition.rows().enumerate().skip(1) {
                    let power_state = &row.1[part_power_column].get_string_value(
                        "vParition - column 'Powerstate'".to_string(),
                        row.0 + 1,
                    )?;

                    if power_state.contains("poweredOff") && !cli.include_powered_off {
                        continue;
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

                    part_vec.push(Vpartition {
                        vm_name: vm_name.to_string(),
                        capacity: *cap,
                    })
                }
            }
        } else {
            if !cli.do_not_use_vpartition {
                println!(
                    "vPartition sheet not found in {:?}, continuing without it.",
                    cli.rvtools_files[i]
                );
            }
        }
    }

    Ok((info_vec, part_vec))
}
