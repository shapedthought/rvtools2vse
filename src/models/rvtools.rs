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
    /// Per-file join identity: `<file index>#<vInfo row>`.
    #[serde(skip)]
    pub vm_key: String,
    /// Identity used to tell same-named VMs apart (Instance UUID, else VM UUID).
    #[serde(skip)]
    pub vm_uuid: String,
    /// Part of `capacity` (MiB) on disks image-level backup cannot protect.
    #[serde(skip)]
    pub non_vadp_capacity: f64,
}

#[derive(Debug, Clone)]
pub struct Vpartition {
    pub vm_name: String,
    pub vm_key: String,
    /// Consumed MiB (used for sizing).
    pub capacity: f64,
    /// Partition size (Capacity MiB), used by the in-guest storage heuristic.
    pub size: f64,
    /// vPartition `Disk Key` (empty when unknown).
    pub disk_key: String,
    /// vPartition `Disk` (guest mount path or drive letter).
    pub mount: String,
}

#[derive(Debug, Clone)]
pub struct Vdisk {
    pub vm_key: String,
    pub capacity: f64,
    pub disk_label: String,
    pub raw_comp_mode: String,
    pub disk_mode: String,
    pub sharing: String,
    pub disk_uuid: String,
    pub is_physical_rdm: bool,
    pub is_independent: bool,
    pub is_shared: bool,
    /// vDisk `Shared Bus` (noSharing / physicalSharing / virtualSharing; empty = unknown).
    pub shared_bus: String,
    /// vDisk `Raw LUN ID` for RDMs.
    pub raw_lun_id: String,
    /// vDisk `VI SDK Server`, scoping LUN IDs to a vCenter.
    pub vi_sdk: String,
    /// Disk sits on a SCSI controller with bus sharing (WSFC-style).
    pub is_shared_bus: bool,
}

impl Vdisk {
    fn uuid(&self) -> Option<String> {
        let uuid = self.disk_uuid.trim();
        (!uuid.is_empty() && !uuid.eq_ignore_ascii_case("none")).then(|| uuid.to_ascii_lowercase())
    }

    fn lun(&self) -> Option<String> {
        let lun = self.raw_lun_id.trim();
        (!lun.is_empty() && !lun.eq_ignore_ascii_case("none")).then(|| lun.to_ascii_lowercase())
    }

    /// Whether this disk may be the same storage attached to several VMs.
    /// Multi-writer disks always; shared-bus disks only with a UUID or LUN ID,
    /// so unrelated disks are never merged on a label/size guess.
    pub fn dedupes(&self) -> bool {
        self.is_shared || (self.is_shared_bus && (self.uuid().is_some() || self.lun().is_some()))
    }

    /// Image-level (VADP) backup cannot protect this disk.
    pub fn is_non_vadp(&self) -> bool {
        self.is_physical_rdm || self.is_independent || self.is_shared || self.is_shared_bus
    }

    /// Stable key for shared-disk de-duplication across VMs: Disk UUID, else
    /// Raw LUN ID scoped to the vCenter (physical RDMs often have no UUID).
    pub fn share_key(&self) -> String {
        if let Some(uuid) = self.uuid() {
            format!("uuid:{uuid}")
        } else if let Some(lun) = self.lun() {
            format!("lun:{}:{lun}", self.vi_sdk.trim().to_ascii_lowercase())
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
