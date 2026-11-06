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
    /// Optional vInfo `Tools` column (toolsOk / toolsNotRunning / …).
    #[serde(skip)]
    pub tools_status: Option<String>,
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
    pub disk_mode: String,
    pub sharing: String,
    pub disk_uuid: String,
    pub is_physical_rdm: bool,
    pub is_independent: bool,
    pub is_shared: bool,
}

impl Vdisk {
    /// Stable key for shared-disk de-duplication across VMs.
    pub fn share_key(&self) -> String {
        if !self.disk_uuid.is_empty() && self.disk_uuid != "None" {
            format!("uuid:{}", self.disk_uuid)
        } else {
            format!("cap:{:.0}:{}", self.capacity, self.disk_label)
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Datacenter {
    pub name: String,
    pub cluster: String,
    pub vm_count: usize,
    pub capacity: f64,
}
