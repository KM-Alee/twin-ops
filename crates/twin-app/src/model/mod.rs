mod doctor_result;
mod graph_result;
mod init_result;
mod scan_result;

pub use doctor_result::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, PermissionMode,
};
pub use graph_result::{
    edge_summary, node_summary, GraphEdgeSummary, GraphListResult, GraphNodeResult,
    GraphNodeSummary, GraphParentEdge, GraphResult,
};
pub use init_result::InitResult;
pub use scan_result::{ScanResult, ScanWarning, ScanWarningDetail};
