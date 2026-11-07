//! `--export-parquet`: write the cleaned, joined per-VM and per-disk tables.

use std::{collections::HashMap, fs, path::Path, sync::Arc};

use anyhow::{Context, Result};
use arrow_array::{ArrayRef, BooleanArray, Float64Array, RecordBatch, StringArray};
use arrow_schema::{DataType, Field, Schema};
use parquet::{arrow::ArrowWriter, basic::Compression, file::properties::WriterProperties};

use crate::models::rvtools::{Vdisk, Vinfo};

pub struct Options<'a> {
    pub files: &'a [std::path::PathBuf],
    pub anonymize: bool,
    pub keep_rdm: bool,
    pub keep_independent: bool,
}

/// Writes `vms.parquet` and `disks.parquet` into `dir` (created if missing).
/// Returns the written paths.
pub fn write(
    dir: &Path,
    vms: &[Vinfo],
    disks: &[Vdisk],
    opts: &Options,
) -> Result<(std::path::PathBuf, std::path::PathBuf)> {
    fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let hide = |s: &str| {
        if opts.anonymize {
            crate::anon(s)
        } else {
            s.to_string()
        }
    };
    let source = |key: &str| -> String {
        let name = key
            .split('#')
            .next()
            .and_then(|i| i.parse::<usize>().ok())
            .and_then(|i| opts.files.get(i))
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        hide(&name)
    };

    let s = |v: Vec<String>| Arc::new(StringArray::from(v)) as ArrayRef;
    let os = |v: Vec<Option<String>>| Arc::new(StringArray::from(v)) as ArrayRef;
    let f = |v: Vec<f64>| Arc::new(Float64Array::from(v)) as ArrayRef;
    let of = |v: Vec<Option<f64>>| Arc::new(Float64Array::from(v)) as ArrayRef;
    let b = |v: Vec<bool>| Arc::new(BooleanArray::from(v)) as ArrayRef;

    let vm_batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("source_file", DataType::Utf8, false),
            Field::new("datacenter", DataType::Utf8, false),
            Field::new("cluster", DataType::Utf8, false),
            Field::new("vm_name", DataType::Utf8, false),
            Field::new("vm_uuid", DataType::Utf8, false),
            Field::new("powerstate", DataType::Utf8, false),
            Field::new("capacity_mib", DataType::Float64, false),
            Field::new("vpartition_mib", DataType::Float64, true),
            Field::new("tools_status", DataType::Utf8, true),
            Field::new("non_vadp_mib", DataType::Float64, false),
        ])),
        vec![
            s(vms.iter().map(|v| source(&v.vm_key)).collect()),
            s(vms.iter().map(|v| hide(&v.datacenter)).collect()),
            s(vms.iter().map(|v| hide(&v.cluster)).collect()),
            s(vms.iter().map(|v| hide(&v.vm_name)).collect()),
            s(vms
                .iter()
                .map(|v| {
                    if v.vm_uuid.is_empty() {
                        String::new()
                    } else {
                        hide(&v.vm_uuid)
                    }
                })
                .collect()),
            s(vms.iter().map(|v| v.powerstate.clone()).collect()),
            f(vms.iter().map(|v| v.capacity).collect()),
            of(vms.iter().map(|v| v.vpartition_capacity).collect()),
            os(vms.iter().map(|v| v.tools_status.clone()).collect()),
            f(vms.iter().map(|v| v.non_vadp_capacity).collect()),
        ],
    )?;

    // Only disks that belong to a VM that survived filters.
    let by_key: HashMap<&str, &Vinfo> = vms.iter().map(|v| (v.vm_key.as_str(), v)).collect();
    let disks: Vec<(&Vdisk, &Vinfo)> = disks
        .iter()
        .filter_map(|d| by_key.get(d.vm_key.as_str()).map(|v| (d, *v)))
        .collect();
    let disk_batch = RecordBatch::try_new(
        Arc::new(Schema::new(vec![
            Field::new("source_file", DataType::Utf8, false),
            Field::new("datacenter", DataType::Utf8, false),
            Field::new("cluster", DataType::Utf8, false),
            Field::new("vm_name", DataType::Utf8, false),
            Field::new("disk_label", DataType::Utf8, false),
            Field::new("disk_uuid", DataType::Utf8, false),
            Field::new("capacity_mib", DataType::Float64, false),
            Field::new("disk_mode", DataType::Utf8, false),
            Field::new("raw_comp_mode", DataType::Utf8, false),
            Field::new("sharing", DataType::Utf8, false),
            Field::new("is_physical_rdm", DataType::Boolean, false),
            Field::new("is_independent", DataType::Boolean, false),
            Field::new("is_shared", DataType::Boolean, false),
            Field::new("excluded_by_flags", DataType::Boolean, false),
            Field::new("shared_bus", DataType::Utf8, false),
            Field::new("raw_lun_id", DataType::Utf8, false),
            Field::new("coverage", DataType::Utf8, false),
        ])),
        vec![
            s(disks.iter().map(|(d, _)| source(&d.vm_key)).collect()),
            s(disks.iter().map(|(_, v)| hide(&v.datacenter)).collect()),
            s(disks.iter().map(|(_, v)| hide(&v.cluster)).collect()),
            s(disks.iter().map(|(_, v)| hide(&v.vm_name)).collect()),
            s(disks.iter().map(|(d, _)| d.disk_label.clone()).collect()),
            s(disks
                .iter()
                .map(|(d, _)| {
                    if d.disk_uuid.is_empty() {
                        String::new()
                    } else {
                        hide(&d.disk_uuid)
                    }
                })
                .collect()),
            f(disks.iter().map(|(d, _)| d.capacity).collect()),
            s(disks.iter().map(|(d, _)| d.disk_mode.clone()).collect()),
            s(disks.iter().map(|(d, _)| d.raw_comp_mode.clone()).collect()),
            s(disks.iter().map(|(d, _)| d.sharing.clone()).collect()),
            b(disks.iter().map(|(d, _)| d.is_physical_rdm).collect()),
            b(disks.iter().map(|(d, _)| d.is_independent).collect()),
            b(disks.iter().map(|(d, _)| d.is_shared).collect()),
            b(disks
                .iter()
                .map(|(d, _)| {
                    (d.is_physical_rdm && !opts.keep_rdm)
                        || (d.is_independent && !opts.keep_independent)
                })
                .collect()),
            s(disks.iter().map(|(d, _)| d.shared_bus.clone()).collect()),
            s(disks
                .iter()
                .map(|(d, _)| {
                    if d.raw_lun_id.is_empty() {
                        String::new()
                    } else {
                        hide(&d.raw_lun_id)
                    }
                })
                .collect()),
            s(disks
                .iter()
                .map(|(d, _)| {
                    if d.is_non_vadp() {
                        "agent_or_other".to_string()
                    } else {
                        "image_level".to_string()
                    }
                })
                .collect()),
        ],
    )?;

    let vm_path = dir.join("vms.parquet");
    let disk_path = dir.join("disks.parquet");
    write_batch(&vm_path, &vm_batch)?;
    write_batch(&disk_path, &disk_batch)?;
    Ok((vm_path, disk_path))
}

fn write_batch(path: &Path, batch: &RecordBatch) -> Result<()> {
    let file = fs::File::create(path).with_context(|| format!("creating {}", path.display()))?;
    let props = WriterProperties::builder()
        .set_compression(Compression::SNAPPY)
        .build();
    let mut writer = ArrowWriter::try_new(file, batch.schema(), Some(props))?;
    writer.write(batch)?;
    writer.close()?;
    Ok(())
}
