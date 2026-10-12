use std::path::PathBuf;

use serde::Serialize;
use twin_k8s::{WatchSummary, WatchUpdate};

use crate::error::{AppError, K8sCmdError};
use crate::paths::{resolve_command_paths, TwinLayout};

#[derive(Debug, Clone)]
pub struct K8sWatchRequest {
    pub duration_seconds: u64,
    pub max_events: usize,
    pub config_override: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct K8sWatchResult {
    pub duration_seconds: u64,
    pub summary: WatchSummary,
}

pub fn run_home(request: K8sWatchRequest) -> Result<K8sWatchResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request)
}

pub fn run(layout: &TwinLayout, request: &K8sWatchRequest) -> Result<K8sWatchResult, AppError> {
    run_configured(layout, request, None)
}

pub fn run_configured(
    layout: &TwinLayout,
    request: &K8sWatchRequest,
    config: Option<twin_k8s::ClientConfig>,
) -> Result<K8sWatchResult, AppError> {
    twin_k8s::validate_watch_limits(request.duration_seconds, request.max_events)
        .map_err(K8sCmdError::Read)?;
    crate::config_io::load(layout, request.config_override.as_deref())?;
    let mut persistence_error = None;
    let callback = |update: WatchUpdate<'_>| {
        let result = match update {
            WatchUpdate::View(view) => super::scan::persist(layout, view),
            WatchUpdate::Gap(detail) => super::scan::persist_gap(layout, detail),
        };
        match result {
            Ok(()) => Ok(()),
            Err(error) => {
                persistence_error = Some(error);
                Err(twin_k8s::K8sError::Client {
                    detail: "cannot persist Kubernetes watch read".into(),
                })
            }
        }
    };
    let summary = match config {
        Some(config) => twin_k8s::watch_cluster_configured(
            config,
            request.duration_seconds,
            request.max_events,
            callback,
        ),
        None => twin_k8s::watch_cluster(request.duration_seconds, request.max_events, callback),
    };
    if let Some(error) = persistence_error {
        return Err(error);
    }
    if summary.is_err() {
        super::scan::persist_gap(
            layout,
            "Kubernetes watch source unavailable; previous graph preserved",
        )?;
    }
    Ok(K8sWatchResult {
        duration_seconds: request.duration_seconds,
        summary: summary.map_err(K8sCmdError::Read)?,
    })
}
