use crate::helpers;
use helpers::{ColPosition, GetFloat, GetString};
use calamine::{open_workbook, Reader, Xlsx};

use crate::models::{cli::Cli, rvtools::{Vinfo, Vpartition}};

pub fn get_excel(cli: &Cli) -> Result<(Vec<Vinfo>, Vec<Vpartition>), anyhow::Error> {
    let mut excel: Xlsx<_> = open_workbook(cli.rvtools_file.clone())?;

    let workbook = excel.worksheet_range("vInfo").unwrap().unwrap();

    let vm_column = workbook.get_col_pos(&"VM".to_string())?;

    let power_column = workbook.get_col_pos(&"Powerstate".to_string())?;

    let cap_column = workbook.get_col_pos(&"In Use MiB".to_string())?;

    let dc_column = workbook.get_col_pos(&"Datacenter".to_string())?;

    let cluster_column = workbook.get_col_pos(&"Cluster".to_string())?;

    let partition = excel.worksheet_range("vPartition").unwrap().unwrap();

    let part_vm_column = partition.get_col_pos(&"VM".to_string())?;

    let part_power_column = partition.get_col_pos(&"Powerstate".to_string())?;

    let part_cap_column = partition.get_col_pos(&"Consumed MiB".to_string())?;

    let mut info_vec: Vec<Vinfo> = Vec::new();
    for row in workbook.rows().enumerate().skip(1) {
        
        let power_state = &row.1[power_column].get_string_value("vInfo - column Powerstate vInfo".to_string(),
            row.0 + 1,
        )?;
        
        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }


        let vm_name = &row.1[vm_column].get_string_value("vInfo - column 'VM'".to_string(),
            row.0 + 1,
        )?;

        let cap = &row.1[cap_column].get_float_value("vInfo - column 'Capacity MiB'".to_string(),
            row.0 + 1,
        )?;

        let dc = &row.1[dc_column].get_string_value("vInfo - column 'Datacenter'".to_string(),
            row.0 + 1,
        )?;

        let cluster = &row.1[cluster_column].get_string_value("vInfo - column 'Cluster'".to_string(),
            row.0 + 1,
        )?;

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
    let mut part_vec: Vec<Vpartition> = Vec::new();
    for row in partition.rows().enumerate().skip(1) {
        
        let power_state = &row.1[part_power_column].get_string_value("vParition - column 'Powerstate'".to_string(), row.0 + 1)?;

        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }
 
        let vm_name = &row.1[part_vm_column].get_string_value("vParition - column 'VM'".to_string(), row.0 + 1)?;

        let cap = &row.1[part_cap_column].get_float_value("vParition - column 'Capacity MiB'".to_string(), row.0 + 1)?;
        

        part_vec.push(Vpartition {
            vm_name: vm_name.to_string(),
            capacity: *cap,
        })
    }
    Ok((info_vec, part_vec))
}
