use std::time::Duration;

use futures_util::io::AsyncReadExt;
use k8s_openapi::api::core::v1::Pod;
use kube::api::LogParams;
use kube::{Api, Config};
use serde::Serialize;

use crate::K8sError;

#[derive(Debug, Clone)]
pub struct PodLogRequest {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub previous: bool,
    pub tail_lines: i64,
    pub max_bytes: usize,
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct PodLogSummary {
    pub namespace: String,
    pub pod: String,
    pub container: Option<String>,
    pub previous: bool,
    pub received_bytes: usize,
    pub line_breaks: usize,
    pub phrase_matches: Vec<crate::log_signals::LogSignalCount>,
    pub max_bytes: usize,
    pub tail_lines: i64,
    pub limit_reached: bool,
    pub coverage_gap: Option<String>,
}

pub fn read_pod_log_summary(request: &PodLogRequest) -> Result<PodLogSummary, K8sError> {
    read_sync(None, request)
}

pub fn read_pod_log_summary_configured(
    config: Config,
    request: &PodLogRequest,
) -> Result<PodLogSummary, K8sError> {
    read_sync(Some(config), request)
}

fn read_sync(config: Option<Config>, request: &PodLogRequest) -> Result<PodLogSummary, K8sError> {
    validate(request)?;
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(error(
            "use read_pod_log_summary_async inside an async runtime",
        ));
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| error("cannot initialize the read runtime"))?;
    runtime.block_on(async {
        let config = match config {
            Some(config) => config,
            None => crate::live::infer_config().await?,
        };
        read_pod_log_summary_async(config, request).await
    })
}

pub async fn read_pod_log_summary_async(
    config: Config,
    request: &PodLogRequest,
) -> Result<PodLogSummary, K8sError> {
    validate(request)?;
    let client = crate::live::client_from_config(config)?;
    let api: Api<Pod> = Api::namespaced(client, &request.namespace);
    let params = LogParams {
        container: request.container.clone(),
        previous: request.previous,
        tail_lines: Some(request.tail_lines),
        limit_bytes: Some(request.max_bytes as i64 + 1),
        ..LogParams::default()
    };
    let mut summary = empty_summary(request);
    let mut classifier = crate::log_signals::LogClassifier::new();
    let read = async {
        let mut stream = api
            .log_stream(&request.pod, &params)
            .await
            .map_err(|error| crate::live::read_error(error).to_string())?;
        let mut buffer = [0_u8; 4096];
        loop {
            let remaining = request.max_bytes - summary.received_bytes;
            if remaining == 0 {
                summary.limit_reached = true;
                return Ok::<_, String>(());
            }
            let count = stream
                .read(&mut buffer[..remaining.min(4096)])
                .await
                .map_err(|_| "log stream interrupted".to_string())?;
            if count == 0 {
                return Ok(());
            }
            classifier.add(&buffer[..count]);
            summary.received_bytes += count;
            summary.line_breaks += buffer[..count]
                .iter()
                .filter(|byte| **byte == b'\n')
                .count();
            buffer[..count].fill(0);
        }
    };
    match tokio::time::timeout(Duration::from_secs(request.timeout_seconds), read).await {
        Ok(Ok(())) => {}
        Ok(Err(detail)) => summary.coverage_gap = Some(detail),
        Err(_) => summary.coverage_gap = Some("log read timed out; summary is partial".into()),
    }
    summary.phrase_matches = classifier.finish();
    Ok(summary)
}

pub fn validate(request: &PodLogRequest) -> Result<(), K8sError> {
    if !crate::watch::safe_name(&request.namespace)
        || !crate::watch::safe_name(&request.pod)
        || request
            .container
            .as_deref()
            .is_some_and(|name| !crate::watch::safe_name(name))
    {
        return Err(error(
            "pod log target must use valid Kubernetes resource names",
        ));
    }
    if !(1..=10_000).contains(&request.tail_lines)
        || !(1..=1_048_576).contains(&request.max_bytes)
        || !(1..=60).contains(&request.timeout_seconds)
    {
        return Err(error(
            "logs require 1-10000 tail lines, 1-1048576 bytes and 1-60 seconds",
        ));
    }
    Ok(())
}

fn empty_summary(request: &PodLogRequest) -> PodLogSummary {
    PodLogSummary {
        namespace: request.namespace.clone(),
        pod: request.pod.clone(),
        container: request.container.clone(),
        previous: request.previous,
        received_bytes: 0,
        line_breaks: 0,
        phrase_matches: Vec::new(),
        max_bytes: request.max_bytes,
        tail_lines: request.tail_lines,
        limit_reached: false,
        coverage_gap: None,
    }
}
fn error(detail: &str) -> K8sError {
    K8sError::Client {
        detail: detail.into(),
    }
}
