use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ScanWarning {
    pub kind: String,
    pub count: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanWarningDetail {
    pub kind: String,
    pub path: String,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanResult {
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub process_count: usize,
    pub parent_edge_count: usize,
    pub cgroup_count: usize,
    pub service_count: usize,
    pub in_cgroup_edge_count: usize,
    pub service_owns_edge_count: usize,
    pub observation_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<ScanWarning>,
    pub warning_details: Vec<ScanWarningDetail>,
}
