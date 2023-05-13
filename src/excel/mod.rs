use helpers::{MyRange, GetStringValue, GetFloatValue};
use office::Excel;

use crate::{
    helpers::{self},
    models::{
        cli::Cli,
        rvtools::{Vinfo, Vpartition},
    },
};

pub fn get_excel(cli: &Cli) -> Result<(Vec<Vinfo>, Vec<Vpartition>), anyhow::Error> {
    let mut excel = Excel::open(cli.rvtools_file.clone()).unwrap();
    let workbook = excel.worksheet_range("vInfo").unwrap();

    let vm_column = workbook.get_col_position(&"VM".to_string())?;
    let power_column = workbook.get_col_position(&"Powerstate".to_string())?;
    let cap_column = workbook.get_col_position(&"In Use MiB".to_string())?;
    let dc_column = workbook.get_col_position(&"Datacenter".to_string())?;
    let cluster_column = workbook.get_col_position(&"Cluster".to_string())?;
    let partition = excel.worksheet_range("vPartition").unwrap();
    let part_vm_column = partition.get_col_position(&"VM".to_string())?;
    let part_power_column = partition.get_col_position(&"Powerstate".to_string())?;
    let part_cap_column = partition.get_col_position(&"Consumed MiB".to_string())?;

    let mut info_vec: Vec<Vinfo> = Vec::new();
    for row in workbook.rows().skip(1) {
        let power_state = row[power_column].get_string_value("vInfo - column powerState".to_string())?;
        // let power_state =
        //     get_string_value(&row[power_column], "vInfo - column powerState".to_string())?;
        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }
        let vm_name = row[vm_column].get_string_value("vInfo - column 'VM'".to_string())?;
        // let vm_name = get_string_value(&row[vm_column], "vInfo - column 'VM'".to_string())?;
        let cap = row[cap_column].get_float_value("vInfo - column 'Capacity MiB'".to_string())?;
        // let cap = get_float_value(
        //     &row[cap_column],
        //     "vInfo - column 'Capacity MiB'".to_string(),
        // )?;
        let dc = row[dc_column].get_string_value("vInfo - column 'Datacenter'".to_string())?;
        // let dc = get_string_value(&row[dc_column], "vInfo - column 'Datacenter'".to_string())?;
        let cluster =
            row[cluster_column].get_string_value("vInfo - column 'Cluster'".to_string())?;
        // let cluster =
        //     get_string_value(&row[cluster_column], "vInfo - column 'Cluster'".to_string())?;

        info_vec.push(Vinfo {
            vm_name: vm_name.to_string(),
            datacenter: dc.to_string(),
            cluster: cluster.to_string(),
            capacity: cap,
        })
    }
    if let Some(dc_include) = &cli.dc_include {
        info_vec = info_vec
            .into_iter()
            .filter(|x| dc_include.contains(&x.datacenter))
            .collect::<Vec<Vinfo>>();

        println!("Including Datacenters: {:?}", dc_include);
    }
    if let Some(cluster_include) = &cli.cluster_include {
        info_vec = info_vec
            .into_iter()
            .filter(|x| cluster_include.contains(&x.cluster))
            .collect::<Vec<Vinfo>>();

        println!("Including Clusters: {:?}", cluster_include);
    }
    if let Some(dc_exclude) = &cli.dc_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !dc_exclude.contains(&x.datacenter))
            .collect::<Vec<Vinfo>>();

        println!("Excluding Datacenters: {:?}", dc_exclude);
    }
    if let Some(cluster_exclude) = &cli.cluster_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !cluster_exclude.contains(&x.cluster))
            .collect::<Vec<Vinfo>>();

        println!("Excluding Clusters: {:?}", cluster_exclude);
    }
    if let Some(vm_exclude) = &cli.vm_exclude {
        info_vec = info_vec
            .into_iter()
            .filter(|x| !vm_exclude.contains(&x.vm_name))
            .collect::<Vec<Vinfo>>();

        println!("Excluding VMs: {:?}", vm_exclude);
    }
    let mut part_vec: Vec<Vpartition> = Vec::new();
    for row in partition.rows().skip(1) {
        let power_state = row[part_power_column].get_string_value("vParition - column 'powerState'".to_string())?;
        // let power_state = get_string_value(
        //     &row[part_power_column],
        //     "vParition - column 'powerState'".to_string(),
        // )?;
        if power_state.contains("poweredOff") && !cli.include_powered_off {
            continue;
        }
        let vm_name = row[part_vm_column].get_string_value("vParition - column 'VM'".to_string())?;
        // let vm_name =
        //     get_string_value(&row[part_vm_column], "vParition - column 'VM'".to_string())?;
        let cap = row[part_cap_column].get_float_value("vParition - column 'Capacity MiB'".to_string())?;
        // let cap = get_float_value(
        //     &row[part_cap_column],
        //     "vParition - column 'Capacity MiB'".to_string(),
        // )?;

        part_vec.push(Vpartition {
            vm_name: vm_name.to_string(),
            capacity: cap,
        })
    }
    Ok((info_vec, part_vec))
}
