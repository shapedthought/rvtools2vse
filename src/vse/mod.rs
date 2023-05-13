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
        .map(|x| Site {
            id: x.to_string(),
            name: x.to_string(),
        })
        .collect::<Vec<Site>>();

    // performance tier repos
    let repos = datacenter_strings
        .iter()
        .map(|x| PerfTierRepo {
            repo_id: format!("{}_repo", x),
            repo_name: format!("{}_repo", x),
            site_id: x.to_string(),
            copy_capacity_tier_enabled: false,
            move_capacity_tier_enabled: false,
            archive_tier_enabled: false,
            capacity_tier_days: 0,
            archive_tier_days: 0,
            capacity_tier_repo_id: "general-s3compatible-capacity".to_string(),
            archive_tier_repo_id: "general-glacier-archive".to_string(),
            storage_type: "xfsRefs".to_string(),
            immutable_cap: false,
            immutable_perf: false,
        })
        .collect::<Vec<PerfTierRepo>>();

    let cap_tier = CapArchTier {
        id: "general-s3compatible-capacity".to_string(),
        tier_type: "Capacity".to_string(),
        name: "General S3 compatible".to_string(),
        default: true,
    };

    let arch_tier = CapArchTier {
        id: "general-glacier-archive".to_string(),
        tier_type: "Archive".to_string(),
        name: "General Amazon S3 Glacier".to_string(),
        default: true,
    };

    let data_property = DataProperty {
        data_property_id: "dpopt".to_string(),
        data_property_name: "Generic Optimistic".to_string(),
        change_rate: 5,
        compression: 50,
        growth_factor: 10,
        default: true,
    };

    let window = Window {
        backup_window_id: "bw12".to_string(),
        backup_window_name: "backup_window1".to_string(),
        full_window: 24,
        incremental_window: 12,
        default: true,
    };

    let retention = Retentions {
        retention_id: "rt1".to_string(),
        retention_name: "30D".to_string(),
        simple: 30,
        weekly: 0,
        monthly: 0,
        yearly: 0,
        default: true,
    };

    let workloads = datacenters
        .iter()
        .map(|x| {
            let backup = Backup {
                retention_id: "rt1".to_string(),
                repo_id: format!("{}_repo", x.name),
                backup_window_id: "bw12".to_string(),
            };

            Workload {
                workload_id: format!("{}_workload", x.cluster),
                enabled: true,
                workload_name: format!("{}_workload", x.cluster),
                site_id: x.name.to_string(),
                large_block: false,
                source_tb: x.capacity,
                units: x.vm_count as i64,
                workload_type: "VM".to_string(),
                data_property_id: "dpopt".to_string(),
                backup,
                copies_enabled: false,
                copies: None,
            }
        })
        .collect::<Vec<Workload>>();

    Ok(NewVse {
        project_length: 3,
        sites,
        repositories: repos,
        cap_arch_tiers: vec![cap_tier, arch_tier],
        data_properties: vec![data_property],
        windows: vec![window],
        retentions: vec![retention],
        workloads,
    })
}
