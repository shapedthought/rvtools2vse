use anyhow::Result;

use crate::models::{
    new_model::{
        Backup, CapArchTier, DataProperty, NewVse, PerfTierRepo, Retentions, Site, Window, Workload,
    },
    rvtools::Datacenter,
};

pub fn vse_construct(
    datacenter_strings: Vec<String>,
    datacenters: &Vec<Datacenter>,
) -> Result<NewVse> {
    let sites = datacenter_strings
        .iter()
        .map(|x| Site::new(x.to_string(), x.to_string()))
        .collect::<Vec<Site>>();

    // performance tier repos
    let repos = datacenter_strings
        .iter()
        .map(|x| {
            PerfTierRepo::new(
                format!("{}_repo", x),
                format!("{}_repo", x),
                x.to_string(),
                false,
                false,
                false,
                0,
                0,
                "general-s3compatible-capacity".to_string(),
                "general-glacier-archive".to_string(),
                "xfsRefs".to_string(),
                false,
                false,
                false,
            )
        })
        .collect::<Vec<PerfTierRepo>>();

    let cap_tier = CapArchTier::new(
        "general-s3compatible-capacity".to_string(),
        "Capacity".to_string(),
        "General S3 compatible".to_string(),
        true,
    );

    let arch_tier = CapArchTier::new(
        "general-glacier-archive".to_string(),
        "Archive".to_string(),
        "General Amazon S3 Glacier".to_string(),
        true,
    );

    let data_property = DataProperty::new(
        "dpopt".to_string(),
        "Generic Optimistic".to_string(),
        5,
        50,
        10,
        true,
    );

    let window = Window::new(
        "bw12".to_string(),
        "backup_window1".to_string(),
        24,
        12,
        true,
    );

    let retention = Retentions::new(
        "rt1".to_string(),
        "30D".to_string(),
        "Instance".to_string(),
        30,
        0,
        0,
        0,
        true,
    );

    let workloads = datacenters
        .iter()
        .map(|x| {
            let backup = Backup::new(
                "rt1".to_string(),
                format!("{}_repo", x.name),
                "bw12".to_string(),
            );

            let copies = Backup::new("".to_string(), "".to_string(), "".to_string());

            Workload::new(
                format!("{}_workload", x.cluster),
                true,
                format!("{}_workload", x.cluster),
                x.name.to_string(),
                false,
                x.capacity,
                x.vm_count as i64,
                "VM".to_string(),
                "dpopt".to_string(),
                backup,
                false,
                copies,
            )
        })
        .collect::<Vec<Workload>>();

    Ok(NewVse::new(
        3,
        sites,
        repos,
        vec![cap_tier, arch_tier],
        vec![data_property],
        vec![window],
        vec![retention],
        workloads,
        vec![],
        "Millions".to_string(),
    ))
}
