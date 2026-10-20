use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::str::FromStr;

use twin_core::{EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind, NodeState};
use twin_store::Store;

use crate::error::{AppError, GraphError};
use crate::model::{
    BriefFact, BriefItem, BriefRestart, BriefResult, BriefSection, GraphOwnedNode, GraphResult,
};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::{EmulateActionRequest, EmulateRequest, GraphRequest, WhatChangedRequest};

const SECTION_LIMIT: usize = 12;
const CHANGE_LIMIT: usize = 8;

#[derive(Debug, Clone)]
pub struct BriefRequest {
    pub target: Option<String>,
    pub config_override: Option<PathBuf>,
    pub max_graph_age_secs: u64,
    pub changes_since: String,
}

pub fn run_home(request: BriefRequest) -> Result<BriefResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request, Path::new("/proc"))
}

pub fn run(
    layout: &TwinLayout,
    request: &BriefRequest,
    proc_root: &Path,
) -> Result<BriefResult, AppError> {
    crate::config_io::load(layout, request.config_override.as_deref())?;
    if !layout.db_file().exists() {
        return Err(GraphError::DatabaseNotInitialized.into());
    }
    let graph_age_secs = super::preflight::refresh_if_stale(
        layout,
        request.config_override.as_deref(),
        request.max_graph_age_secs,
        proc_root,
    )?;
    let store = Store::open(&layout.db_file()).map_err(GraphError::StoreOpen)?;
    let coverage_notes = super::coverage::load(&store)?
        .unknown_lines()
        .into_iter()
        .map(|line| line.detail)
        .collect();
    let mut result = match request.target.as_deref() {
        None => host_brief(layout, request, &store, proc_root)?,
        Some(target) => target_brief(layout, request, &store, target)?,
    };
    result.graph_age_secs = graph_age_secs;
    result.coverage_notes = coverage_notes;
    Ok(result)
}

fn section(title: &str, mut items: Vec<BriefItem>) -> Option<BriefSection> {
    if items.is_empty() {
        return None;
    }
    let omitted = items.len().saturating_sub(SECTION_LIMIT);
    items.truncate(SECTION_LIMIT);
    Some(BriefSection {
        title: title.to_string(),
        items,
        omitted,
    })
}

fn fact(label: &str, value: impl Into<String>) -> BriefFact {
    BriefFact {
        label: label.to_string(),
        value: value.into(),
    }
}

fn item(id: Option<&str>, label: &str, detail: impl Into<String>) -> BriefItem {
    BriefItem {
        id: id.map(str::to_string),
        label: label.to_string(),
        detail: detail.into(),
    }
}

fn metadata(node: &GraphNode) -> serde_json::Value {
    serde_json::from_str(node.metadata().as_str()).unwrap_or_default()
}

fn meta_str<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    value.get(key).and_then(|v| v.as_str())
}

fn active_nodes(store: &Store, kind: NodeKind) -> Result<Vec<GraphNode>, AppError> {
    Ok(store
        .list_nodes_by_kind_typed(kind)
        .map_err(GraphError::Store)?
        .into_iter()
        .filter(|node| node.state() == NodeState::Active)
        .collect())
}

fn active_edges(store: &Store, kind: EdgeKind) -> Result<Vec<GraphEdge>, AppError> {
    Ok(store
        .list_active_edges_by_kind(&kind.to_string())
        .map_err(GraphError::Store)?
        .iter()
        .filter_map(|row| GraphEdge::try_from(row).ok())
        .collect())
}

fn read_trimmed(path: PathBuf) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|text| text.trim().to_string())
        .filter(|text| !text.is_empty())
}

fn host_brief(
    layout: &TwinLayout,
    request: &BriefRequest,
    store: &Store,
    proc_root: &Path,
) -> Result<BriefResult, AppError> {
    let services = active_nodes(store, NodeKind::Service)?;
    let labels: HashMap<String, String> = services
        .iter()
        .map(|node| (node.id().to_string(), node.label().to_string()))
        .collect();
    let processes = active_nodes(store, NodeKind::Process)?;
    let containers = active_nodes(store, NodeKind::Container)?;
    let hostname = read_trimmed(proc_root.join("sys/kernel/hostname"))
        .unwrap_or_else(|| "this host".to_string());

    let mut active = 0usize;
    let mut failed = Vec::new();
    for service in &services {
        match meta_str(&metadata(service), "active_state") {
            Some("active") => active += 1,
            Some("failed") => {
                failed.push(item(Some(service.id().as_str()), service.label(), "failed"))
            }
            _ => {}
        }
    }
    let running_containers = containers
        .iter()
        .filter(|node| metadata(node).get("is_running").and_then(|v| v.as_bool()) == Some(true))
        .collect::<Vec<_>>();

    let mut exposed: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut listener_count = 0usize;
    for edge in active_edges(store, EdgeKind::ListensOn)? {
        if edge.to().kind() != Some(NodeKind::Port) {
            continue;
        }
        let endpoint = edge.to().as_str().trim_start_matches("port:").to_string();
        let owners = exposed.entry(endpoint).or_default();
        if owners.is_empty() {
            listener_count += 1;
        }
        let owner = labels
            .get(edge.from().as_str())
            .cloned()
            .unwrap_or_else(|| edge.from().to_string());
        if !owners.contains(&owner) {
            if edge.from().kind() == Some(NodeKind::Service) {
                owners.insert(0, owner);
            } else {
                owners.push(owner);
            }
        }
    }
    let mut by_owner: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (endpoint, owners) in &exposed {
        if super::preflight::is_loopback_endpoint(endpoint) {
            continue;
        }
        let owner = owners
            .first()
            .cloned()
            .unwrap_or_else(|| "owner hidden (permissions)".to_string());
        by_owner.entry(owner).or_default().push(endpoint.clone());
    }
    let exposed_items = by_owner
        .iter()
        .map(|(owner, endpoints)| {
            let shown = endpoints
                .iter()
                .take(4)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let more = endpoints.len().saturating_sub(4);
            let detail = if more > 0 {
                format!("{shown} (+{more} more)")
            } else {
                shown
            };
            item(None, owner, detail)
        })
        .collect::<Vec<_>>();

    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    for kind in [EdgeKind::DependsOn, EdgeKind::ConnectsTo] {
        for edge in active_edges(store, kind)? {
            if edge.class() != twin_core::EdgeClass::Inferred
                || edge.from().kind() != Some(NodeKind::Service)
                || edge.to().kind() != Some(NodeKind::Service)
            {
                continue;
            }
            let entry = dependents.entry(edge.to().to_string()).or_default();
            if !entry.contains(&edge.from().to_string()) {
                entry.push(edge.from().to_string());
            }
        }
    }
    let mut hubs = dependents.into_iter().collect::<Vec<_>>();
    hubs.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then_with(|| a.0.cmp(&b.0)));
    let hub_items = hubs
        .iter()
        .map(|(id, users)| {
            let label = labels.get(id).cloned().unwrap_or_else(|| id.clone());
            item(
                Some(id),
                &label,
                format!("{} runtime dependent(s)", users.len()),
            )
        })
        .collect::<Vec<_>>();

    let images: HashMap<String, String> = active_edges(store, EdgeKind::RunsImage)?
        .into_iter()
        .map(|edge| {
            (
                edge.from().to_string(),
                edge.to().as_str().trim_start_matches("image:").to_string(),
            )
        })
        .collect();
    let container_items = running_containers
        .iter()
        .map(|node| {
            item(
                Some(node.id().as_str()),
                node.label(),
                images.get(node.id().as_str()).cloned().unwrap_or_default(),
            )
        })
        .collect::<Vec<_>>();

    let changes = recent_changes(layout, request, None)?;

    let mut facts = vec![
        fact("host", hostname.clone()),
        fact(
            "services",
            format!("{active} active, {} failed", failed.len()),
        ),
        fact("processes", processes.len().to_string()),
        fact("listening endpoints", listener_count.to_string()),
        fact(
            "containers",
            format!("{} running", running_containers.len()),
        ),
    ];
    if let Some(kernel) = read_trimmed(proc_root.join("sys/kernel/osrelease")) {
        facts.insert(1, fact("kernel", kernel));
    }
    let mut next_steps = Vec::new();
    if let Some((id, _)) = hubs.first() {
        next_steps.push(format!("twin brief {id}"));
    }
    next_steps.push("twin preflight \"<command you plan to run>\"".into());
    next_steps.push("twin what-changed --since 1h".into());
    Ok(BriefResult {
        target: None,
        title: hostname,
        kind: "host".into(),
        facts,
        restart: None,
        sections: [
            section("exposed to the network", exposed_items),
            section("failed units", failed),
            section("most depended-on services", hub_items),
            section("running containers", container_items),
            section(&format!("changes since {}", request.changes_since), changes),
        ]
        .into_iter()
        .flatten()
        .collect(),
        graph_age_secs: 0,
        coverage_notes: Vec::new(),
        next_steps,
    })
}

fn recent_changes(
    layout: &TwinLayout,
    request: &BriefRequest,
    target: Option<&str>,
) -> Result<Vec<BriefItem>, AppError> {
    use twin_core::ChangeCategory;
    let changes = super::what_changed::run(
        layout,
        &WhatChangedRequest {
            config_override: request.config_override.clone(),
            since: request.changes_since.clone(),
            verbose: false,
            target_query: target.map(str::to_string),
            categories: Vec::new(),
            limit: 10_000,
            max_depth: 1,
        },
    )?;
    // Process and outbound churn is constant on a desktop or busy server; count it
    // instead of letting it hide restarts, config edits and package changes.
    let is_churn = |category: ChangeCategory| {
        matches!(
            category,
            ChangeCategory::ProcessStarted
                | ChangeCategory::ProcessDisappeared
                | ChangeCategory::OutboundEndpointAdded
        )
    };
    let significant = changes
        .operational_changes
        .iter()
        .filter(|change| !is_churn(change.category))
        .collect::<Vec<_>>();
    let mut items = significant
        .iter()
        .rev()
        .take(CHANGE_LIMIT)
        .map(|change| {
            item(
                Some(change.entity.as_str()),
                &format!("{} · {}", change.recorded_at_label(), change.category),
                change.observation.clone(),
            )
        })
        .collect::<Vec<_>>();
    let older =
        significant.len().saturating_sub(CHANGE_LIMIT) + changes.omitted_operational_changes;
    if older > 0 {
        items.push(item(None, "older", format!("{older} more change(s)")));
    }
    let mut churn: BTreeMap<String, usize> = BTreeMap::new();
    for change in &changes.operational_changes {
        if is_churn(change.category) {
            *churn.entry(change.category.to_string()).or_default() += 1;
        }
    }
    if !churn.is_empty() {
        let summary = churn
            .iter()
            .map(|(category, count)| format!("{count} {category}"))
            .collect::<Vec<_>>()
            .join(", ");
        items.push(item(None, "routine churn", summary));
    }
    Ok(items)
}

fn owned(nodes: &[GraphOwnedNode]) -> Vec<BriefItem> {
    let mut items: Vec<BriefItem> = Vec::new();
    for node in nodes {
        if items
            .iter()
            .any(|known| known.id.as_deref() == Some(node.id.as_str()))
        {
            continue;
        }
        let detail = node.tag.clone().unwrap_or_else(|| node.edge_class.clone());
        items.push(item(Some(&node.id), &node.label, detail));
    }
    items
}

fn graph_request(request: &BriefRequest, target: &str) -> GraphRequest {
    let (target_id, target_query) = if target.starts_with('/') {
        (Some(NodeId::file(target)), None)
    } else if let Ok(pid) = target.parse::<u32>() {
        (Some(NodeId::process(pid)), None)
    } else if let Some(id) = NodeId::from_str(target)
        .ok()
        .filter(|id| id.kind().is_some())
    {
        (Some(id), None)
    } else {
        (None, Some(target.to_string()))
    };
    GraphRequest {
        config_override: request.config_override.clone(),
        target: target_id,
        target_query,
        ..GraphRequest::default()
    }
}

fn target_brief(
    layout: &TwinLayout,
    request: &BriefRequest,
    store: &Store,
    target: &str,
) -> Result<BriefResult, AppError> {
    let mut query = graph_request(request, target);
    if query.target.is_none()
        && super::resolve_service::resolve_service_target(
            store,
            target,
            super::resolve_service::ServiceNotFoundContext::Emulate,
        )
        .is_err()
    {
        let container = NodeId::container(target);
        if store
            .get_node_typed(&container)
            .map_err(GraphError::Store)?
            .is_some()
        {
            query.target = Some(container);
            query.target_query = None;
        }
    }
    let graph = super::graph::run(layout, &query)?;
    let (id, title, kind, mut facts, sections) = match &graph {
        GraphResult::Service(r) => {
            let node = store
                .get_node_typed(&NodeId::service(
                    r.service.id.trim_start_matches("service:"),
                ))
                .map_err(GraphError::Store)?;
            let meta = node.as_ref().map(metadata).unwrap_or_default();
            let state = match (
                meta_str(&meta, "active_state"),
                meta_str(&meta, "sub_state"),
            ) {
                (Some(active), Some(sub)) => format!("{active} ({sub})"),
                (Some(active), None) => active.to_string(),
                _ if !r.owned_processes.is_empty() => {
                    "running (inferred from its processes)".to_string()
                }
                _ => "unknown".to_string(),
            };
            let pids = r
                .owned_processes
                .iter()
                .take(6)
                .map(|p| p.id.trim_start_matches("process:pid:").to_string())
                .collect::<Vec<_>>();
            let mut facts = vec![fact("state", state)];
            if !pids.is_empty() {
                facts.push(fact(
                    "processes",
                    format!("{} (pids {})", r.owned_processes.len(), pids.join(", ")),
                ));
            }
            let mut listens = owned(&r.listening_ports);
            listens.extend(owned(&r.listening_unix));
            let mut connects = owned(&r.connected_ports);
            connects.extend(owned(&r.connected_unix));
            let managed = if r.service.label == "docker.service" {
                active_nodes(store, NodeKind::Container)?
                    .iter()
                    .filter(|node| {
                        let meta = metadata(node);
                        meta_str(&meta, "source") == Some("docker")
                            && meta.get("is_running").and_then(|v| v.as_bool()) == Some(true)
                    })
                    .map(|node| item(Some(node.id().as_str()), node.label(), "running"))
                    .collect()
            } else {
                Vec::new()
            };
            let sections = vec![
                section("manages containers", managed),
                section("listens on", listens),
                section("depended on by", owned(&r.dependents)),
                section("depends on", owned(&r.dependencies)),
                section("connects to", connects),
                section(
                    "declared dependents (no runtime use seen)",
                    owned(&r.configured_dependents),
                ),
                section("configuration files", owned(&r.configured_files)),
                section("socket activation", owned(&r.socket_activation)),
                section("timer activation", owned(&r.timer_activation)),
            ];
            (
                r.service.id.clone(),
                r.service.label.clone(),
                "service".to_string(),
                facts,
                sections,
            )
        }
        GraphResult::Container(r) => {
            let node = store.get_node(&r.container.id).map_err(GraphError::Store)?;
            let meta: serde_json::Value = node
                .map(|row| serde_json::from_str(&row.metadata_json).unwrap_or_default())
                .unwrap_or_default();
            let mut facts = Vec::new();
            if let Some(image) = &r.image {
                facts.push(fact("image", image.label.clone()));
            }
            match meta.get("is_running").and_then(|v| v.as_bool()) {
                Some(true) => facts.push(fact("state", "running")),
                Some(false) => facts.push(fact("state", "stopped")),
                None => facts.push(fact("state", "unknown")),
            }
            if let Some(health) = meta_str(&meta, "health_status").filter(|h| *h != "none") {
                facts.push(fact("health", health));
            }
            match meta.get("daemon_live_restore").and_then(|v| v.as_bool()) {
                Some(true) => facts.push(fact("daemon restart", "survives (live-restore)")),
                Some(false) => facts.push(fact("daemon restart", "stops this container")),
                None => {}
            }
            let ports = r
                .ports
                .iter()
                .map(|p| {
                    item(
                        Some(&p.port_id),
                        &format!("{}:{}", p.host_ip, p.host_port),
                        format!("→ container port {}", p.container_port),
                    )
                })
                .collect();
            let sections = vec![
                section("published ports", ports),
                section("volumes", owned(&r.mounts)),
                section("processes", owned(&r.processes)),
            ];
            (
                r.container.id.clone(),
                r.container.label.clone(),
                "container".to_string(),
                facts,
                sections,
            )
        }
        GraphResult::Port(r) => {
            let mut listeners = owned(&r.service_listeners);
            listeners.extend(owned(&r.process_listeners));
            let mut callers = owned(&r.service_callers);
            callers.extend(owned(&r.process_callers));
            let exposure =
                if super::preflight::is_loopback_endpoint(r.port.id.trim_start_matches("port:")) {
                    "local only (loopback)"
                } else {
                    "reachable from the network"
                };
            (
                r.port.id.clone(),
                r.port.label.clone(),
                "port".to_string(),
                vec![fact("exposure", exposure)],
                vec![section("listeners", listeners), section("clients", callers)],
            )
        }
        GraphResult::File(r) => (
            r.file.id.clone(),
            r.file.label.clone(),
            "file".to_string(),
            Vec::new(),
            vec![
                section("configures", owned(&r.configures)),
                section("references", owned(&r.references)),
            ],
        ),
        GraphResult::Package(r) => (
            r.package.id.clone(),
            r.package.label.clone(),
            "package".to_string(),
            Vec::new(),
            vec![
                section("services loading its libraries", owned(&r.services)),
                section("libraries", owned(&r.libraries)),
            ],
        ),
        GraphResult::Node(r) => {
            let out = r
                .outgoing
                .iter()
                .map(|e| item(Some(&e.peer_id), &e.peer_label, e.kind.clone()))
                .collect();
            let incoming = r
                .incoming
                .iter()
                .map(|e| item(Some(&e.peer_id), &e.peer_label, e.kind.clone()))
                .collect();
            let kind = NodeId::from_str(&r.node.id)
                .ok()
                .and_then(|id| id.kind())
                .map(|kind| kind.to_string())
                .unwrap_or_else(|| "node".into());
            (
                r.node.id.clone(),
                r.node.label.clone(),
                kind,
                Vec::new(),
                vec![section("outgoing", out), section("incoming", incoming)],
            )
        }
        other => {
            let (id, label) = match other {
                GraphResult::Mount(r) => (r.mount.id.clone(), r.mount.label.clone()),
                GraphResult::Directory(r) => (r.directory.id.clone(), r.directory.label.clone()),
                GraphResult::Library(r) => (r.library.id.clone(), r.library.label.clone()),
                GraphResult::UnixSocket(r) => {
                    (r.unix_socket.id.clone(), r.unix_socket.label.clone())
                }
                _ => (target.to_string(), target.to_string()),
            };
            (id, label, "node".to_string(), Vec::new(), Vec::new())
        }
    };
    let restart = match kind.as_str() {
        "service" | "container" => Some(restart_summary(layout, request, &id)),
        _ => None,
    };
    if kind == "service" {
        facts.push(fact("id", id.clone()));
    }
    let changes = recent_changes(layout, request, Some(&id)).unwrap_or_default();
    let mut sections = sections.into_iter().flatten().collect::<Vec<_>>();
    if let Some(changes) = section(&format!("changes since {}", request.changes_since), changes) {
        sections.push(changes);
    }
    let mut next_steps = vec![format!("twin why {id}")];
    if let Some(example) = super::preflight::restart_example(&kind, &title) {
        next_steps.push(format!("twin preflight \"{example}\""));
    }
    Ok(BriefResult {
        target: Some(id),
        title,
        kind,
        facts,
        restart,
        sections,
        graph_age_secs: 0,
        coverage_notes: Vec::new(),
        next_steps,
    })
}

fn restart_summary(layout: &TwinLayout, request: &BriefRequest, id: &str) -> BriefRestart {
    let action = EmulateActionRequest::Restart {
        target: NodeId::from_str(id).ok(),
        target_query: None,
        show_paths: false,
        max_depth: crate::DEFAULT_MAX_DEPTH,
    };
    let emulated = super::emulate::run(
        layout,
        &EmulateRequest {
            config_override: request.config_override.clone(),
            action,
            show_evidence: false,
        },
    );
    match emulated {
        Ok(result) => {
            let step = super::preflight::from_emulation("", String::new(), &result, false);
            BriefRestart {
                verdict: step.verdict,
                summary: step
                    .reasons
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "no dependents observed".into()),
            }
        }
        Err(error) => BriefRestart {
            verdict: crate::model::PreflightVerdict::Caution,
            summary: format!("could not emulate a restart: {error}"),
        },
    }
}
