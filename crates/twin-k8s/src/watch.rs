use std::time::Duration;

use futures_util::io::{AsyncBufRead, AsyncBufReadExt, AsyncReadExt};
use kube::api::{
    ApiResource, DynamicObject, GroupVersionKind, ListParams, PartialObjectMeta, Request,
    WatchEvent, WatchParams,
};
use kube::{Api, Client, Config};
use serde::Serialize;
use tokio::time::Instant;

use crate::{ClusterView, K8sError};

#[derive(Debug, Clone, Serialize)]
pub struct WatchNotice {
    pub resource: String,
    pub event: String,
    pub name: Option<String>,
    pub namespace: Option<String>,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct WatchSummary {
    pub observed_events: usize,
    pub refreshes: usize,
    pub reconnects: usize,
    pub relists: usize,
    pub notices: Vec<WatchNotice>,
    pub omitted_notices: usize,
    pub warnings: Vec<String>,
    pub omitted_warnings: usize,
    pub limit_reached: bool,
}

pub enum WatchUpdate<'a> {
    View(&'a ClusterView),
    Gap(&'a str),
}

pub fn watch_cluster(
    seconds: u64,
    max_events: usize,
    callback: impl FnMut(WatchUpdate<'_>) -> Result<(), K8sError>,
) -> Result<WatchSummary, K8sError> {
    watch_sync(None, seconds, max_events, callback)
}

pub fn watch_cluster_configured(
    config: Config,
    seconds: u64,
    max_events: usize,
    callback: impl FnMut(WatchUpdate<'_>) -> Result<(), K8sError>,
) -> Result<WatchSummary, K8sError> {
    watch_sync(Some(config), seconds, max_events, callback)
}

fn watch_sync(
    config: Option<Config>,
    seconds: u64,
    max_events: usize,
    callback: impl FnMut(WatchUpdate<'_>) -> Result<(), K8sError>,
) -> Result<WatchSummary, K8sError> {
    validate_watch_limits(seconds, max_events)?;
    if tokio::runtime::Handle::try_current().is_ok() {
        return Err(error("use watch_cluster_async inside an async runtime"));
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
        watch_cluster_async(config, seconds, max_events, callback).await
    })
}

pub async fn watch_cluster_async(
    config: Config,
    seconds: u64,
    max_events: usize,
    mut callback: impl FnMut(WatchUpdate<'_>) -> Result<(), K8sError>,
) -> Result<WatchSummary, K8sError> {
    validate_watch_limits(seconds, max_events)?;
    let client = crate::live::client_from_config(config.clone())?;
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
    let mut tasks = tokio::task::JoinSet::new();
    for &(group, version, kind, plural, _) in crate::live::RESOURCES {
        tasks.spawn(watch_resource(
            client.clone(),
            group,
            version,
            kind,
            plural,
            deadline,
            sender.clone(),
        ));
    }
    drop(sender);
    let mut summary = WatchSummary::default();
    refresh(&config, deadline, &mut summary, &mut callback).await?;
    while let Ok(Some(signal)) = tokio::time::timeout_at(deadline, receiver.recv()).await {
        match signal {
            Signal::Gap(detail) => {
                warning(&mut summary, &detail);
                callback(WatchUpdate::Gap(&detail))?;
            }
            Signal::Notice(notice) => {
                match notice.event.as_str() {
                    "reconnected" => summary.reconnects += 1,
                    "relisted" => summary.relists += 1,
                    _ => summary.observed_events += 1,
                }
                let should_refresh = notice.event != "reconnected";
                if summary.notices.len() < 256 {
                    summary.notices.push(notice);
                } else {
                    summary.omitted_notices += 1;
                }
                if should_refresh {
                    refresh(&config, deadline, &mut summary, &mut callback).await?;
                }
                if summary.observed_events >= max_events {
                    summary.limit_reached = true;
                    break;
                }
            }
        }
    }
    tasks.abort_all();
    while tasks.join_next().await.is_some() {}
    Ok(summary)
}

async fn refresh(
    config: &Config,
    deadline: Instant,
    summary: &mut WatchSummary,
    callback: &mut impl FnMut(WatchUpdate<'_>) -> Result<(), K8sError>,
) -> Result<(), K8sError> {
    match tokio::time::timeout_at(deadline, crate::read_cluster_async(config.clone())).await {
        Ok(Ok(view)) => {
            callback(WatchUpdate::View(&view))?;
            summary.refreshes += 1;
        }
        _ => {
            let detail = "Kubernetes watch refresh unavailable; previous graph preserved";
            warning(summary, detail);
            callback(WatchUpdate::Gap(detail))?;
        }
    }
    Ok(())
}

fn warning(summary: &mut WatchSummary, detail: &str) {
    if summary.warnings.iter().any(|existing| existing == detail) {
        return;
    }
    if summary.warnings.len() < 64 {
        summary.warnings.push(detail.into());
    } else {
        summary.omitted_warnings += 1;
    }
}

enum Signal {
    Notice(WatchNotice),
    Gap(String),
}

async fn watch_resource(
    client: Client,
    group: &'static str,
    version: &'static str,
    kind: &'static str,
    plural: &'static str,
    deadline: Instant,
    sender: tokio::sync::mpsc::Sender<Signal>,
) {
    let mut resource = ApiResource::from_gvk(&GroupVersionKind::gvk(group, version, kind));
    resource.plural = plural.into();
    let api: Api<DynamicObject> = Api::all_with(client.clone(), &resource);
    let mut resource_version = None;
    let mut attempts = 0;
    while Instant::now() < deadline {
        if resource_version.is_none() {
            match tokio::time::timeout_at(deadline, list_version(&api)).await {
                Ok(Ok(version)) => {
                    resource_version = Some(version);
                    if sender
                        .send(Signal::Notice(notice(kind, "relisted", None)))
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                _ => {
                    if sender
                        .send(Signal::Gap(format!(
                            "Kubernetes {plural}: watch list unavailable"
                        )))
                        .await
                        .is_err()
                    {
                        return;
                    }
                    pause(deadline, attempts).await;
                    attempts += 1;
                    continue;
                }
            }
        }
        let params = WatchParams::default().timeout(5);
        let version = resource_version.as_deref().unwrap_or("0");
        let request = match Request::new(api.resource_url()).watch_metadata(&params, version) {
            Ok(request) => request,
            Err(_) => return,
        };
        let response = tokio::time::timeout_at(deadline, client.request_stream(request)).await;
        let mut stream = match response {
            Ok(Ok(stream)) => stream,
            Ok(Err(kube::Error::Api(status))) if status.code == 410 => {
                resource_version = None;
                pause(deadline, attempts).await;
                attempts += 1;
                continue;
            }
            _ => {
                if sender
                    .send(Signal::Gap(format!(
                        "Kubernetes {plural}: watch interrupted"
                    )))
                    .await
                    .is_err()
                {
                    return;
                }
                pause(deadline, attempts).await;
                attempts += 1;
                continue;
            }
        };
        if attempts > 0
            && sender
                .send(Signal::Notice(notice(kind, "reconnected", None)))
                .await
                .is_err()
        {
            return;
        }
        while let Ok(Ok(Some(event))) =
            tokio::time::timeout_at(deadline, read_event(&mut stream)).await
        {
            let (event, object) = match event {
                WatchEvent::Added(object) => ("added", object),
                WatchEvent::Modified(object) => ("modified", object),
                WatchEvent::Deleted(object) => ("deleted", object),
                WatchEvent::Bookmark(bookmark) => {
                    resource_version = Some(bookmark.metadata.resource_version);
                    continue;
                }
                WatchEvent::Error(status) if status.code == 410 => {
                    resource_version = None;
                    break;
                }
                _ => {
                    break;
                }
            };
            if let Some(version) = &object.metadata.resource_version {
                resource_version = Some(version.clone());
            }
            if sender
                .send(Signal::Notice(notice(kind, event, Some(&object.metadata))))
                .await
                .is_err()
            {
                return;
            }
            attempts = 0;
        }
        if Instant::now() < deadline {
            if sender
                .send(Signal::Gap(format!(
                    "Kubernetes {plural}: watch interrupted; reconnect or relist required"
                )))
                .await
                .is_err()
            {
                return;
            }
            pause(deadline, attempts).await;
            attempts += 1;
        }
    }
}

async fn list_version(api: &Api<DynamicObject>) -> Result<String, K8sError> {
    let mut params = ListParams::default().limit(250);
    let mut version = None;
    for _ in 0..1000 {
        let list = api
            .list_metadata(&params)
            .await
            .map_err(|_| error("watch list unavailable"))?;
        if version.is_none() {
            version = list.metadata.resource_version;
        }
        let Some(next) = list.metadata.continue_.filter(|next| !next.is_empty()) else {
            return version
                .filter(|version| !version.is_empty())
                .ok_or_else(|| error("list omitted resource version"));
        };
        if params.continue_token.as_ref() == Some(&next) {
            return Err(error("watch pagination did not advance"));
        }
        params = params.continue_token(&next);
    }
    Err(error("watch pagination limit reached"))
}

async fn pause(deadline: Instant, attempts: u32) {
    let delay = Duration::from_millis(100_u64.saturating_mul(1_u64 << attempts.min(4)));
    let _ = tokio::time::timeout_at(deadline, tokio::time::sleep(delay)).await;
}

fn notice(
    kind: &str,
    event: &str,
    metadata: Option<&k8s_openapi::apimachinery::pkg::apis::meta::v1::ObjectMeta>,
) -> WatchNotice {
    WatchNotice {
        resource: kind.into(),
        event: event.into(),
        name: metadata
            .and_then(|m| m.name.as_deref())
            .filter(|name| safe_name(name))
            .map(str::to_string),
        namespace: metadata
            .and_then(|m| m.namespace.as_deref())
            .filter(|name| safe_name(name))
            .map(str::to_string),
    }
}

pub(crate) fn safe_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 253
        && name.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.'
        })
}

pub fn validate_watch_limits(seconds: u64, max_events: usize) -> Result<(), K8sError> {
    if !(1..=3600).contains(&seconds) || !(1..=4096).contains(&max_events) {
        return Err(error("watch requires 1-3600 seconds and 1-4096 events"));
    }
    Ok(())
}
fn error(detail: &str) -> K8sError {
    K8sError::Client {
        detail: detail.into(),
    }
}

async fn read_event(
    stream: &mut (impl AsyncBufRead + Unpin),
) -> Result<Option<WatchEvent<PartialObjectMeta<DynamicObject>>>, &'static str> {
    let mut line = Vec::new();
    let count = stream
        .take(65_537)
        .read_until(b'\n', &mut line)
        .await
        .map_err(|_| "watch read interrupted")?;
    if count == 0 {
        return Ok(None);
    }
    if line.len() > 65_536 {
        return Err("watch event exceeded byte bound");
    }
    serde_json::from_slice(&line)
        .map(Some)
        .map_err(|_| "malformed watch event")
}
