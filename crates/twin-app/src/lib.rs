mod commands;
mod config_io;
mod error;
mod model;
pub mod paths;

pub use error::{AppError, GraphError, ScanError};
pub use model::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, GraphEdgeSummary, GraphListResult,
    GraphNodeResult, GraphNodeSummary, GraphParentEdge, GraphResult, InitResult, PermissionMode,
    ScanResult, ScanWarning, ScanWarningDetail,
};
pub use paths::TwinLayout;

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

#[derive(Debug, Clone, Default)]
pub struct ScanRequest {
    pub config_override: Option<PathBuf>,
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
}

pub fn graph(request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run_home(request)
}

pub fn graph_in(layout: &TwinLayout, request: GraphRequest) -> Result<GraphResult, AppError> {
    commands::graph::run(layout, &request)
}
