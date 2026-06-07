mod doctor_result;
mod emulation_result;
mod graph_result;
mod impact_result;
mod init_result;
mod scan_quality;
mod scan_result;

pub use doctor_result::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, PermissionMode,
};
pub use emulation_result::{
    EmulationImpact, EmulationImpactPathView, EmulationOverlayNode, EmulationOverlaySummary,
    EmulationResult,
};
pub use graph_result::{
    edge_summary, node_summary, GraphEdgeSummary, GraphEvidenceLine, GraphFileResult,
    GraphListResult, GraphNodeResult, GraphNodeSummary, GraphOwnedNode, GraphParentEdge,
    GraphPortResult, GraphResult, GraphServiceResult, GraphUnixSocketResult,
};
pub use impact_result::{
    EvidenceStrengthView, ImpactDependent, ImpactEvidenceLine, ImpactNodeSummary, ImpactPath,
    ImpactPathStep, ImpactResult, ImpactUnknown, RiskAssessment,
};
pub use init_result::InitResult;
pub use scan_quality::{ScanQuality, ScanQualityAssessment};
pub use scan_result::{ScanResult, ScanWarning, ScanWarningDetail};
