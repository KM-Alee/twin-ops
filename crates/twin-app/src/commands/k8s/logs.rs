use std::path::PathBuf;

use twin_k8s::{PodLogRequest, PodLogSummary};

use crate::error::{AppError, K8sCmdError};
use crate::paths::{resolve_command_paths, TwinLayout};

#[derive(Debug, Clone)]
pub struct K8sLogsRequest {
    pub pod: PodLogRequest,
    pub config_override: Option<PathBuf>,
}

pub fn run_home(request: K8sLogsRequest) -> Result<PodLogSummary, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request)
}

pub fn run(layout: &TwinLayout, request: &K8sLogsRequest) -> Result<PodLogSummary, AppError> {
    run_configured(layout, request, None)
}

pub fn run_configured(
    layout: &TwinLayout,
    request: &K8sLogsRequest,
    config: Option<twin_k8s::ClientConfig>,
) -> Result<PodLogSummary, AppError> {
    crate::config_io::load(layout, request.config_override.as_deref())?;
    match config {
        Some(config) => twin_k8s::read_pod_log_summary_configured(config, &request.pod),
        None => twin_k8s::read_pod_log_summary(&request.pod),
    }
    .map_err(K8sCmdError::Read)
    .map_err(AppError::from)
}
