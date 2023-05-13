#[derive(Debug, Clone)]
pub struct Vinfo {
    pub vm_name: String,
    pub datacenter: String,
    pub cluster: String,
    pub capacity: f64,
}

#[derive(Debug, Clone)]
pub struct Vpartition {
    pub vm_name: String,
    pub capacity: f64,
}

#[derive(Debug, Clone)]
pub struct Datacenter {
    pub name: String,
    pub cluster: String,
    pub vm_count: usize,
    pub capacity: f64,
}