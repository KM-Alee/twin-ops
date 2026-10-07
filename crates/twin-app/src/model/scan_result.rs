use serde::Serialize;

use super::coverage::CoverageReport;

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

#[derive(Debug, Clone, Serialize, Default)]
pub struct ScanResult {
    pub samples_requested: u32,
    pub samples_completed: u32,
    pub interval_secs: u64,
    pub edges_first_seen_by_sample: Vec<usize>,
    pub socket_activation_edge_count: usize,
    pub cgroup_correction_count: usize,
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub process_count: usize,
    pub parent_edge_count: usize,
    pub cgroup_count: usize,
    pub service_count: usize,
    pub in_cgroup_edge_count: usize,
    pub service_owns_edge_count: usize,
    pub tcp_listener_count: usize,
    pub port_count: usize,
    pub process_listens_on_edge_count: usize,
    pub service_listens_on_edge_count: usize,
    pub unmapped_listener_socket_count: usize,
    pub tcp_connection_count: usize,
    pub process_connects_to_edge_count: usize,
    pub service_connects_to_edge_count: usize,
    pub service_depends_on_edge_count: usize,
    pub declared_depends_on_edge_count: usize,
    pub systemd_unit_count: usize,
    pub enable_depends_on_edge_count: usize,
    pub dbus_depends_on_edge_count: usize,
    pub dbus_available: bool,
    pub unix_listener_count: usize,
    pub unix_socket_count: usize,
    pub process_listens_on_unix_edge_count: usize,
    pub service_listens_on_unix_edge_count: usize,
    pub unmapped_unix_listener_count: usize,
    pub unix_connection_count: usize,
    pub process_connects_to_unix_edge_count: usize,
    pub service_connects_to_unix_edge_count: usize,
    pub service_depends_on_unix_edge_count: usize,
    pub unmapped_active_socket_count: usize,
    pub socket_owner_inode_count: usize,
    pub observation_count: usize,
    pub warning_count: usize,
    pub warnings: Vec<ScanWarning>,
    pub warning_details: Vec<ScanWarningDetail>,
    pub coverage: CoverageReport,
}
