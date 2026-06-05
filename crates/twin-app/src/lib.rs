mod commands;
mod config_io;
mod error;
mod model;
pub mod paths;

pub use error::{AppError, GraphError, ImpactError, ScanError};
pub use model::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, EvidenceStrengthView,
    GraphEdgeSummary, GraphEvidenceLine, GraphListResult, GraphNodeResult, GraphNodeSummary,
    GraphOwnedNode, GraphParentEdge, GraphPortResult, GraphResult, GraphServiceResult,
    GraphUnixSocketResult, ImpactDependent, ImpactEvidenceLine, ImpactResult, ImpactUnknown,
    InitResult, PermissionMode, RiskAssessment, ScanQuality, ScanQualityAssessment, ScanResult,
    ScanWarning, ScanWarningDetail,
};
pub use paths::TwinLayout;

pub use crate::commands::scan_quality::assess_scan_quality;

use std::path::{Path, PathBuf};

use twin_core::{NodeId, NodeKind};

#[derive(Debug, Clone, Default)]
pub struct InitRequest {
    pub force: bool,
    pub config_override: Option<PathBuf>,
}

pub fn init(request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run_home(request)
}

pub fn init_in(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run(layout, request)
}

pub fn doctor(config_override: Option<&std::path::Path>) -> Result<DoctorResult, AppError> {
    commands::doctor::run_home(config_override)
}

pub fn doctor_in(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
) -> Result<DoctorResult, AppError> {
    commands::doctor::run(layout, config_override)
}

pub fn read_config(
    path: &std::path::Path,
) -> Result<twin_core::config::TwinConfig, twin_core::error::ConfigError> {
    config_io::read(path)
}

#[derive(Debug, Clone)]
pub struct ScanRequest {
    pub config_override: Option<PathBuf>,
    pub samples: u32,
    pub interval_secs: u64,
}

impl Default for ScanRequest {
    fn default() -> Self {
        Self {
            config_override: None,
            samples: 1,
            interval_secs: 2,
        }
    }
}

impl ScanRequest {
    pub fn default_with_config(config_override: Option<PathBuf>) -> Self {
        Self {
            config_override,
            ..Self::default()
        }
    }
}

pub fn scan(request: ScanRequest) -> Result<ScanResult, AppError> {
    commands::scan::run_home(request, Path::new("/proc"))
}

pub fn scan_in(
    layout: &TwinLayout,
    request: ScanRequest,
    proc_root: &Path,
) -> Result<ScanResult, AppError> {
    commands::scan::run(layout, &request, proc_root)
}

#[derive(Debug, Clone, Default)]
pub struct GraphRequest {
    pub config_override: Option<PathBuf>,
    pub kind: Option<NodeKind>,
    pub target: Option<NodeId>,
    pub target_query: Option<String>,
}

pub fn graph(request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run_home(request)
}

pub fn graph_in(layout: &TwinLayout, request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run(layout, &request)
}

#[derive(Debug, Clone, Default)]
pub struct ImpactRequest {
    pub config_override: Option<PathBuf>,
    pub target: Option<NodeId>,
    pub target_query: Option<String>,
}

pub fn impact(request: ImpactRequest) -> Result<ImpactResult, AppError> {
    commands::impact::run_home(request)
}

pub fn impact_in(layout: &TwinLayout, request: ImpactRequest) -> Result<ImpactResult, AppError> {
    commands::impact::run(layout, &request)
}
