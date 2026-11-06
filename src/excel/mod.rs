use crate::{helpers, models::errors::MyError};
use calamine::{open_workbook, Data, Range, Reader, Xlsx, XlsxError};
use helpers::{
    as_display_string, is_independent_disk_mode, is_physical_rdm_mode, is_shared_disk_mode,
    ColPosition, GetFloat, GetString,
};
use std::collections::{HashMap, HashSet};

use crate::models::{
    cli::Cli,
    rvtools::{Vdisk, Vinfo, Vpartition},
};

type ExcelSheets = (Vec<Vinfo>, Vec<Vpartition>, Vec<Vdisk>);

pub fn get_excel(cli: &Cli) -> Result<ExcelSheets, MyError> {
    let mut excel_vec: Vec<Xlsx<_>> = Vec::new();

    if cli.rvtools_files.is_empty() {
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

        let vm_column = workbook.get_col_pos("VM")?;

        let power_column = workbook.get_col_pos("Powerstate")?;

        let cap_string = if cli.legacy {
            "In Use MB"
        } else {
            "In Use MiB"
        };

        let cap_column = workbook.get_col_pos(cap_string)?;

        let dc_column = workbook.get_col_pos("Datacenter")?;

        let cluster_column = workbook.get_col_pos("Cluster")?;

        let tools_column = workbook.get_col_pos("Tools").ok();
        let mut file_ids: Vec<(String, String, String)> = Vec::new();
        let info_ids = IdColumns::find(&workbook);

        let mut file_info: Vec<Vinfo> = Vec::new();
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

            let cap =
                &row.1[cap_column].get_float_value(cap_error_string.to_string(), row.0 + 1)?;

            // Real names are kept here; --anonymize hashes after filters/site maps.
            let dc = &row.1[dc_column]
                .get_string_value("vInfo - column 'Datacenter'".to_string(), row.0 + 1)?;

            let cluster = &row.1[cluster_column]
                .get_string_value("vInfo - column 'Cluster'".to_string(), row.0 + 1)?;

            let (instance_uuid, bios_uuid) = info_ids.read(row.1);
            let key = format!("{i}#{}", row.0 + 1);

            let tools_status = tools_column.and_then(|c| {
                let t = as_display_string(&row.1[c]);
                if t.is_empty() || t == "None" {
                    None
                } else {
                    Some(t)
                }
            });

            file_info.push(Vinfo {
                vm_name: vm_name.to_string(),
                datacenter: dc.to_string(),
                cluster: cluster.to_string(),
                capacity: *cap,
                powerstate: power_state.to_string(),
                vpartition_capacity: None,
                tools_status,
                vm_key: key.clone(),
                vm_uuid: if instance_uuid.is_empty() {
                    bios_uuid.clone()
                } else {
                    instance_uuid.clone()
                },
            });
            file_ids.push((key, instance_uuid, bios_uuid));
        }
        let mut info_vec_file = file_info;
        if let Some(dc_include) = &cli.dc_include {
            info_vec_file = info_vec_file
                .into_iter()
                .filter(|x| dc_include.contains(&x.datacenter))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(cluster_include) = &cli.cluster_include {
            info_vec_file = info_vec_file
                .into_iter()
                .filter(|x| cluster_include.contains(&x.cluster))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(dc_exclude) = &cli.dc_exclude {
            info_vec_file = info_vec_file
                .into_iter()
                .filter(|x| !dc_exclude.contains(&x.datacenter))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(cluster_exclude) = &cli.cluster_exclude {
            info_vec_file = info_vec_file
                .into_iter()
                .filter(|x| !cluster_exclude.contains(&x.cluster))
                .collect::<Vec<Vinfo>>();
        }
        if let Some(vm_exclude) = &cli.vm_exclude {
            info_vec_file = info_vec_file
                .into_iter()
                .filter(|x| !vm_exclude.contains(&x.vm_name))
                .collect::<Vec<Vinfo>>();
        }

        let index = JoinIndex::new(&info_vec_file, &file_ids);
        info_vec.extend(info_vec_file);

        // calamine >= 0.20 returns Err(WorksheetNotFound) instead of None for a
        // missing sheet; map it back to Option<Result<_>> to keep behaviour.
        let partition = match excel.worksheet_range("vPartition") {
            Err(XlsxError::WorksheetNotFound(_)) => None,
            other => Some(other),
        };

        if let Some(partition) = partition {
            if cli.do_not_use_vpartition {
                // skip reading partitions
            } else {
                let partition = match partition {
                    Ok(range) => range,
                    Err(_) => {
                        return Err(MyError::VpartitionError(
                            "vPartition sheet not found".to_string(),
                        ));
                    }
                };

                let part_vm_column = partition.get_col_pos("VM")?;

                // Powerstate is optional on vPartition: power state is already
                // filtered on vInfo, and partitions only count when they join to
                // a vInfo VM, so a missing column just skips the extra filter.
                let part_power_column = partition.get_col_pos("Powerstate").ok();
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

                let part_cap_column = partition.get_col_pos(consumed_string)?;
                let part_ids = IdColumns::find(&partition);
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

                    let cap = &row.1[part_cap_column]
                        .get_float_value(cap_error_string.to_string(), row.0 + 1)?;

                    let (inst, bios) = part_ids.read(row.1);
                    part_vec.push(Vpartition {
                        vm_key: index.resolve(i, &inst, &bios, vm_name),
                        vm_name: vm_name.to_string(),
                        capacity: *cap,
                    })
                }
            }
        } else if !cli.do_not_use_vpartition {
            eprintln!(
                "vPartition sheet not found in {:?}, continuing without it.",
                cli.rvtools_files[i]
            );
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
                let disk_vm_column = disk.get_col_pos("VM")?;
                let disk_cap_string = if cli.legacy {
                    "Capacity MB"
                } else {
                    "Capacity MiB"
                };
                let disk_cap_column = disk.get_col_pos(disk_cap_string)?;
                let disk_mode_column = disk.get_col_pos("Raw Comp. Mode").ok();
                if disk_mode_column.is_none() {
                    eprintln!(
                        "vDisk in {:?} has no 'Raw Comp. Mode' column; physical RDMs cannot be detected.",
                        cli.rvtools_files[i]
                    );
                }
                let disk_label_column = disk.get_col_pos("Disk").ok();
                let disk_power_column = disk.get_col_pos("Powerstate").ok();
                let persistence_column = disk
                    .get_col_pos("Disk Mode")
                    .ok()
                    .or_else(|| disk.get_col_pos("Mode").ok());
                let sharing_column = disk
                    .get_col_pos("Sharing")
                    .ok()
                    .or_else(|| disk.get_col_pos("Sharing mode").ok());
                let uuid_column = find_col(&disk, &["Disk UUID"]);
                let disk_ids = IdColumns::find(&disk);

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

                    let disk_mode = persistence_column
                        .map(|c| as_display_string(&row.1[c]))
                        .unwrap_or_default();
                    let disk_mode = if disk_mode == "None" {
                        String::new()
                    } else {
                        disk_mode
                    };
                    let sharing = sharing_column
                        .map(|c| as_display_string(&row.1[c]))
                        .unwrap_or_default();
                    let sharing = if sharing == "None" {
                        String::new()
                    } else {
                        sharing
                    };
                    let disk_uuid = uuid_column
                        .map(|c| as_display_string(&row.1[c]))
                        .unwrap_or_default();
                    let disk_uuid = if disk_uuid == "None" {
                        String::new()
                    } else {
                        disk_uuid
                    };

                    let (inst, bios) = disk_ids.read(row.1);
                    disk_vec.push(Vdisk {
                        vm_key: index.resolve(i, &inst, &bios, vm_name),
                        capacity: *cap,
                        disk_label: label,
                        raw_comp_mode: raw_comp.clone(),
                        disk_mode: disk_mode.clone(),
                        sharing: sharing.clone(),
                        disk_uuid,
                        is_physical_rdm: is_physical_rdm_mode(&raw_comp),
                        is_independent: is_independent_disk_mode(&disk_mode),
                        is_shared: is_shared_disk_mode(&sharing),
                    });
                }
            }
        }
    }

    Ok((info_vec, part_vec, disk_vec))
}

/// Case/whitespace-insensitive header lookup over several candidate names.
fn find_col(range: &Range<Data>, names: &[&str]) -> Option<usize> {
    let header = range.rows().next()?;
    names.iter().find_map(|want| {
        header.iter().position(|h| match h {
            Data::String(s) => s.trim().eq_ignore_ascii_case(want),
            _ => false,
        })
    })
}

fn clean_id(d: &Data) -> String {
    let s = as_display_string(d);
    let s = s.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("none") {
        String::new()
    } else {
        s.to_ascii_lowercase()
    }
}

/// Optional VM identity columns present on vInfo, vPartition and vDisk.
struct IdColumns {
    instance: Option<usize>,
    bios: Option<usize>,
}

impl IdColumns {
    fn find(range: &Range<Data>) -> Self {
        IdColumns {
            instance: find_col(
                range,
                &["VM Instance UUID", "Instance UUID", "InstanceUUID"],
            ),
            bios: find_col(range, &["VM UUID", "SMBIOS UUID", "BIOS UUID", "UUID"]),
        }
    }

    fn read(&self, row: &[Data]) -> (String, String) {
        let get = |c: Option<usize>| c.and_then(|c| row.get(c)).map(clean_id).unwrap_or_default();
        (get(self.instance), get(self.bios))
    }
}

/// Per-file lookup from child-sheet identity to a vInfo record key.
struct JoinIndex {
    instance: HashMap<String, String>,
    bios: HashMap<String, String>,
    name: HashMap<String, String>,
}

impl JoinIndex {
    fn new(infos: &[Vinfo], ids: &[(String, String, String)]) -> Self {
        let kept: HashSet<&str> = infos.iter().map(|v| v.vm_key.as_str()).collect();
        let mut idx = JoinIndex {
            instance: HashMap::new(),
            bios: HashMap::new(),
            name: HashMap::new(),
        };
        for v in infos {
            idx.name
                .entry(v.vm_name.clone())
                .or_insert_with(|| v.vm_key.clone());
        }
        for (key, inst, bios) in ids {
            if kept.contains(key.as_str()) {
                idx.add_ids(key, inst, bios);
            }
        }
        idx
    }

    fn add_ids(&mut self, key: &str, instance: &str, bios: &str) {
        if !instance.is_empty() {
            self.instance
                .entry(instance.to_string())
                .or_insert_with(|| key.to_string());
        }
        if !bios.is_empty() {
            self.bios
                .entry(bios.to_string())
                .or_insert_with(|| key.to_string());
        }
    }

    /// Instance UUID, then VM UUID, then (first record with) the VM name.
    /// Unmatched rows get a key no vInfo record has, so they never join.
    fn resolve(&self, file: usize, instance: &str, bios: &str, name: &str) -> String {
        let unmatched = || format!("{file}#unmatched:{name}");
        if !instance.is_empty() && !self.instance.is_empty() {
            if let Some(k) = self.instance.get(instance) {
                return k.clone();
            }
        }
        if !bios.is_empty() && !self.bios.is_empty() {
            if let Some(k) = self.bios.get(bios) {
                return k.clone();
            }
        }
        // A UUID the file's vInfo knows the scheme for but not the value means a
        // different (filtered or unlisted) VM: never fall back to its name.
        if (!instance.is_empty() && !self.instance.is_empty())
            || (!bios.is_empty() && !self.bios.is_empty())
        {
            return unmatched();
        }
        self.name.get(name).cloned().unwrap_or_else(unmatched)
    }
}
