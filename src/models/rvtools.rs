use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct Vinfo {
    pub vm_name: String,
    pub datacenter: String,
    pub cluster: String,
    pub capacity: f64,
    pub powerstate: String,
    /// Summed vPartition consumed capacity (MiB) when the VM joined to vPartition.
    /// Not serialized so `--print-json` output is unchanged.
    #[serde(skip)]
    pub vpartition_capacity: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct Vpartition {
    pub vm_name: String,
    pub capacity: f64,
}

#[derive(Debug, Clone)]
pub struct Vdisk {
    pub vm_name: String,
    pub capacity: f64,
    pub disk_label: String,
    pub raw_comp_mode: String,
    pub is_physical_rdm: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Datacenter {
    pub name: String,
    pub cluster: String,
    pub vm_count: usize,
    pub capacity: f64,
}
