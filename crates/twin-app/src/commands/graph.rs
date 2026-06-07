use std::collections::HashSet;
use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{EdgeClass, EdgeKind, GraphEdge, NodeId, NodeKind, ObservationId};
use twin_observation::{Observation, ObservationKind};
use twin_store::Store;

use crate::commands::evidence::{
    graph_systemd_dep_line, is_runtime_active_metadata, EvidenceLoadContext, NodeLoadContext,
};
use crate::commands::resolve_service::{resolve_service_target, ServiceNotFoundContext};
use crate::error::{AppError, GraphError};
use crate::model::{
    edge_summary, node_summary, GraphEvidenceLine, GraphNodeSummary, GraphOwnedNode, GraphResult,
    GraphServiceResult,
};
use crate::model::{GraphEdgeSummary, GraphParentEdge};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::GraphRequest;

pub fn run_home(request: GraphRequest) -> Result<GraphResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    graph_at(&paths.layout, &request, &paths.db_path)
}

pub fn run(layout: &TwinLayout, request: &GraphRequest) -> Result<GraphResult, AppError> {
    graph_at(layout, request, &layout.db_file())
}

fn graph_at(
    layout: &TwinLayout,
    request: &GraphRequest,
    db_path: &std::path::Path,
) -> Result<GraphResult, AppError> {
    let _config_path = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(GraphError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(GraphError::StoreOpen)?;
    if !store.is_initialized().map_err(GraphError::Store)? {
        return Err(GraphError::DatabaseNotInitialized.into());
    }

    if let Some(query) = &request.target_query {
        let known = store
            .count_nodes_by_kind_typed(NodeKind::Service)
            .map_err(GraphError::Store)?;
        let target = resolve_service_target(
            &store,
            query,
            ServiceNotFoundContext::Graph {
                detail: service_not_found_detail(known as usize),
            },
        )?;
        return service_neighborhood(&store, &target);
    }

    if let Some(target) = &request.target {
        return match target.kind() {
            Some(NodeKind::Service) => service_neighborhood(&store, target),
            Some(NodeKind::Process) => process_neighborhood(&store, target),
            Some(NodeKind::Port) => port_neighborhood(&store, target),
            Some(NodeKind::UnixSocket) => unix_socket_neighborhood(&store, target),
            Some(NodeKind::File) => file_neighborhood(&store, target),
            Some(kind) => Err(AppError::UnsupportedGraphKind {
                kind: kind.to_string(),
            }),
            None => Err(GraphError::NodeNotFound { id: target.clone() }.into()),
        };
    }

    let Some(kind) = request.kind else {
        return Err(GraphError::MissingQuery.into());
    };
    list_by_kind(&store, kind)
}

fn service_not_found_detail(known_services: usize) -> String {
    if known_services == 0 {
        return "; no services in the database — run `twin scan` first".to_string();
    }
    format!(
        "; {known_services} services in database — try `twin graph --kind service` to list them"
    )
}

fn list_by_kind(store: &Store, kind: NodeKind) -> Result<GraphResult, AppError> {
    let nodes = store
        .list_nodes_by_kind_typed(kind)
        .map_err(GraphError::Store)?;
    let summaries: Vec<_> = nodes
        .iter()
        .map(|n| node_summary(n.id(), n.label()))
        .collect();

    if kind == NodeKind::Service || kind == NodeKind::Port {
        let mut nodes = summaries;
        nodes.sort_by(|a, b| a.id.cmp(&b.id));
        return Ok(GraphResult::list(kind, nodes, Vec::new()));
    }

    if kind != NodeKind::Process {
        return Err(AppError::UnsupportedGraphKind {
            kind: kind.to_string(),
        });
    }

    let labels: std::collections::HashMap<String, String> = summaries
        .iter()
        .map(|n| (n.id.clone(), n.label.clone()))
        .collect();
    let node_ids: HashSet<String> = summaries.iter().map(|n| n.id.clone()).collect();

    let parent_edges = store
        .list_active_edges_by_kind(&EdgeKind::ParentOf.to_string())
        .map_err(GraphError::Store)?
        .into_iter()
        .filter_map(|row| {
            let edge = GraphEdge::try_from(&row).ok()?;
            let parent = edge.from().as_str().to_string();
            let child = edge.to().as_str().to_string();
            if !node_ids.contains(&parent) || !node_ids.contains(&child) {
                return None;
            }
            let child_label = labels.get(&child)?.clone();
            Some(GraphParentEdge {
                parent_id: parent,
                child_id: child,
                child_label,
            })
        })
        .collect();

    Ok(GraphResult::list(kind, summaries, parent_edges))
}

fn process_neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
    let node = store
        .get_node_typed(target)
        .map_err(GraphError::Store)?
        .ok_or_else(|| GraphError::NodeNotFound { id: target.clone() })?;
    if node.kind() != NodeKind::Process {
        return Err(AppError::UnsupportedGraphKind {
            kind: node.kind().to_string(),
        });
    }

    let summary = node_summary(node.id(), node.label());
    let mut evidence_refs = Vec::new();
    let outgoing = peer_parent_edges(store, target, true, &mut evidence_refs)?;
    let incoming = peer_parent_edges(store, target, false, &mut evidence_refs)?;
    evidence_refs.sort();
    evidence_refs.dedup();

    Ok(GraphResult::node(
        summary,
        outgoing,
        incoming,
        evidence_refs,
    ))
}

fn service_neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
    let node = store
        .get_node_typed(target)
        .map_err(GraphError::Store)?
        .ok_or_else(|| GraphError::NodeNotFound { id: target.clone() })?;
    if node.kind() != NodeKind::Service {
        return Err(AppError::UnsupportedGraphKind {
            kind: node.kind().to_string(),
        });
    }

    let summary = node_summary(node.id(), node.label());
    let mut owned_processes = Vec::new();
    let mut owned_cgroups = Vec::new();
    let mut listening_ports = Vec::new();
    let mut listening_unix = Vec::new();
    let mut connected_ports = Vec::new();
    let mut connected_unix = Vec::new();
    let mut dependencies = Vec::new();
    let mut dependents = Vec::new();
    let mut configured_dependents = Vec::new();
    let mut configured_files = Vec::new();
    let mut socket_activation = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    let outgoing = store
        .list_active_edges_from(target.as_str())
        .map_err(GraphError::Store)?;
    for row in outgoing {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        if edge.kind() == EdgeKind::ConfiguredBy && edge.class() == EdgeClass::Observed {
            let peer = edge.to();
            let peer_node = store
                .get_node_typed(peer)
                .map_err(GraphError::Store)?
                .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
            let obs_links = store
                .list_observations_for_edge(edge.id().as_str())
                .map_err(GraphError::Store)?;
            let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
            collect_config_file_evidence(
                store,
                &observation_ids,
                &mut evidence,
                &mut seen_evidence,
            );
            configured_files.push(GraphOwnedNode {
                id: peer.as_str().to_string(),
                label: peer_node.label().to_string(),
                edge_class: edge.class().to_string(),
                tag: None,
                observation_ids,
            });
            continue;
        }
        if edge.kind() == EdgeKind::ConnectsTo && edge.class() == EdgeClass::Inferred {
            let peer = edge.to();
            let peer_node = store
                .get_node_typed(peer)
                .map_err(GraphError::Store)?
                .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
            let obs_links = store
                .list_observations_for_edge(edge.id().as_str())
                .map_err(GraphError::Store)?;
            let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
            collect_connect_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
            let owned = GraphOwnedNode {
                id: peer.as_str().to_string(),
                label: peer_node.label().to_string(),
                edge_class: edge.class().to_string(),
                tag: None,
                observation_ids,
            };
            match peer_node.kind() {
                NodeKind::Port => connected_ports.push(owned),
                NodeKind::UnixSocket => connected_unix.push(owned),
                _ => {}
            }
            continue;
        }
        if edge.kind() == EdgeKind::DependsOn {
            let peer = edge.to();
            let peer_node = store
                .get_node_typed(peer)
                .map_err(GraphError::Store)?
                .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
            let obs_links = store
                .list_observations_for_edge(edge.id().as_str())
                .map_err(GraphError::Store)?;
            let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
            if edge.class() == EdgeClass::Observed && is_socket_activation_edge(&edge) {
                socket_activation.push(GraphOwnedNode {
                    id: peer.as_str().to_string(),
                    label: peer_node.label().to_string(),
                    edge_class: edge.class().to_string(),
                    tag: Some("activates".to_string()),
                    observation_ids,
                });
                continue;
            }
            if edge.class() == EdgeClass::Inferred {
                collect_dependency_connection_evidence(
                    store,
                    &observation_ids,
                    &mut evidence,
                    &mut seen_evidence,
                );
                collect_socket_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
            } else {
                collect_systemd_unit_evidence(
                    store,
                    &observation_ids,
                    &mut evidence,
                    &mut seen_evidence,
                );
            }
            dependencies.push(GraphOwnedNode {
                id: peer.as_str().to_string(),
                label: peer_node.label().to_string(),
                edge_class: edge.class().to_string(),
                tag: Some(dependency_tag(edge.class()).to_string()),
                observation_ids,
            });
            continue;
        }
        if edge.kind() == EdgeKind::ListensOn && edge.class() == EdgeClass::Inferred {
            let peer = edge.to();
            let peer_node = store
                .get_node_typed(peer)
                .map_err(GraphError::Store)?
                .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
            let obs_links = store
                .list_observations_for_edge(edge.id().as_str())
                .map_err(GraphError::Store)?;
            let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
            collect_socket_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
            let owned = GraphOwnedNode {
                id: peer.as_str().to_string(),
                label: peer_node.label().to_string(),
                edge_class: edge.class().to_string(),
                tag: None,
                observation_ids,
            };
            match peer_node.kind() {
                NodeKind::Port => listening_ports.push(owned),
                NodeKind::UnixSocket => listening_unix.push(owned),
                _ => {}
            }
            continue;
        }
        if edge.kind() != EdgeKind::Owns || edge.class() != EdgeClass::Inferred {
            continue;
        }
        let peer = edge.to();
        let peer_node = store
            .get_node_typed(peer)
            .map_err(GraphError::Store)?
            .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(GraphError::Store)?;
        let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
        collect_cgroup_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);

        let owned = GraphOwnedNode {
            id: peer.as_str().to_string(),
            label: peer_node.label().to_string(),
            edge_class: edge.class().to_string(),
            tag: None,
            observation_ids,
        };
        match peer_node.kind() {
            NodeKind::Process => owned_processes.push(owned),
            NodeKind::Cgroup => owned_cgroups.push(owned),
            _ => {}
        }
    }

    let incoming = store
        .list_active_edges_to(target.as_str())
        .map_err(GraphError::Store)?;
    for row in incoming {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        if edge.kind() != EdgeKind::DependsOn {
            continue;
        }
        let peer = edge.from();
        let peer_node = store
            .get_node_typed(peer)
            .map_err(GraphError::Store)?
            .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(GraphError::Store)?;
        let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
        if edge.class() == EdgeClass::Inferred {
            collect_dependency_connection_evidence(
                store,
                &observation_ids,
                &mut evidence,
                &mut seen_evidence,
            );
        } else {
            collect_systemd_unit_evidence(
                store,
                &observation_ids,
                &mut evidence,
                &mut seen_evidence,
            );
        }
        let owned = GraphOwnedNode {
            id: peer.as_str().to_string(),
            label: peer_node.label().to_string(),
            edge_class: edge.class().to_string(),
            tag: Some(dependency_tag(edge.class()).to_string()),
            observation_ids,
        };
        if edge.class() == EdgeClass::Inferred
            || is_runtime_active_metadata(peer_node.metadata().as_str())
        {
            dependents.push(owned);
        } else {
            configured_dependents.push(owned);
        }
    }

    owned_processes.sort_by_key(|n| n.id.clone());
    owned_cgroups.sort_by_key(|n| n.id.clone());
    listening_ports.sort_by_key(|n| n.id.clone());
    listening_unix.sort_by_key(|n| n.id.clone());
    connected_ports.sort_by_key(|n| n.id.clone());
    connected_unix.sort_by_key(|n| n.id.clone());
    dependencies.sort_by_key(|n| n.id.clone());
    dependents.sort_by_key(|n| n.id.clone());
    configured_dependents.sort_by_key(|n| n.id.clone());
    configured_files.sort_by_key(|n| n.id.clone());
    socket_activation.sort_by_key(|n| n.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(GraphResult::service(GraphServiceResult {
        service: summary,
        owned_processes,
        owned_cgroups,
        listening_ports,
        listening_unix,
        connected_ports,
        connected_unix,
        dependencies,
        dependents,
        socket_activation,
        configured_dependents,
        configured_files,
        evidence,
    }))
}

fn file_neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
    let node = store
        .get_node_typed(target)
        .map_err(GraphError::Store)?
        .ok_or_else(|| GraphError::NodeNotFound { id: target.clone() })?;
    if node.kind() != NodeKind::File {
        return Err(AppError::UnsupportedGraphKind {
            kind: node.kind().to_string(),
        });
    }

    let summary = node_summary(node.id(), node.label());
    let mut configures = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    for row in store
        .list_active_edges_to(target.as_str())
        .map_err(GraphError::Store)?
    {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        if edge.kind() != EdgeKind::ConfiguredBy {
            continue;
        }
        let peer = edge.from();
        let peer_node = store
            .get_node_typed(peer)
            .map_err(GraphError::Store)?
            .ok_or_else(|| GraphError::NodeNotFound { id: peer.clone() })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(GraphError::Store)?;
        let observation_ids: Vec<String> = obs_links.into_iter().map(|(id, _)| id).collect();
        collect_config_file_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
        configures.push(GraphOwnedNode {
            id: peer.as_str().to_string(),
            label: peer_node.label().to_string(),
            edge_class: edge.class().to_string(),
            tag: None,
            observation_ids,
        });
    }
    configures.sort_by_key(|n| n.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(GraphResult::file(summary, configures, evidence))
}

fn is_socket_activation_edge(edge: &GraphEdge) -> bool {
    edge.metadata().socket_activation()
}

fn socket_endpoint_neighborhood<F>(
    store: &Store,
    target: &NodeId,
    expected_kind: NodeKind,
    build: F,
) -> Result<GraphResult, AppError>
where
    F: FnOnce(
        GraphNodeSummary,
        Vec<GraphOwnedNode>,
        Vec<GraphOwnedNode>,
        Vec<GraphOwnedNode>,
        Vec<GraphOwnedNode>,
        Vec<GraphEvidenceLine>,
    ) -> GraphResult,
{
    let node = store
        .get_node_typed(target)
        .map_err(GraphError::Store)?
        .ok_or_else(|| GraphError::NodeNotFound { id: target.clone() })?;
    if node.kind() != expected_kind {
        return Err(AppError::UnsupportedGraphKind {
            kind: node.kind().to_string(),
        });
    }

    let summary = node_summary(node.id(), node.label());
    let mut process_listeners = Vec::new();
    let mut service_listeners = Vec::new();
    let mut process_callers = Vec::new();
    let mut service_callers = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    let incoming_rows = store
        .list_active_edges_to(target.as_str())
        .map_err(GraphError::Store)?;
    let mut incoming_edges = Vec::new();
    let mut peer_id_strs = Vec::new();
    let mut obs_id_strs = Vec::new();
    for row in incoming_rows {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        peer_id_strs.push(edge.from().as_str().to_string());
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(GraphError::Store)?;
        for (obs_id, _) in &obs_links {
            obs_id_strs.push(obs_id.clone());
        }
        incoming_edges.push((edge, obs_links));
    }
    let peer_refs: Vec<&str> = peer_id_strs.iter().map(String::as_str).collect();
    let node_ctx = NodeLoadContext::preload_nodes(store, &peer_refs).map_err(GraphError::Store)?;
    let obs_ctx = EvidenceLoadContext::preload_observations(store, &obs_id_strs)
        .map_err(GraphError::Store)?;

    for (edge, obs_links) in incoming_edges {
        if edge.kind() == EdgeKind::ConnectsTo {
            let caller_id = edge.from();
            let caller_node =
                node_ctx
                    .node(caller_id.as_str())
                    .ok_or_else(|| GraphError::NodeNotFound {
                        id: caller_id.clone(),
                    })?;
            let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
            collect_connect_evidence_cached(
                &obs_ctx,
                &observation_ids,
                &mut evidence,
                &mut seen_evidence,
            );
            let caller = GraphOwnedNode {
                id: caller_id.as_str().to_string(),
                label: caller_node.label().to_string(),
                edge_class: edge.class().to_string(),
                tag: None,
                observation_ids,
            };
            match (edge.class(), caller_node.kind()) {
                (EdgeClass::Observed, NodeKind::Process) => process_callers.push(caller),
                (EdgeClass::Inferred, NodeKind::Service) => service_callers.push(caller),
                _ => {}
            }
            continue;
        }
        if edge.kind() != EdgeKind::ListensOn {
            continue;
        }
        let listener_id = edge.from();
        let listener_node =
            node_ctx
                .node(listener_id.as_str())
                .ok_or_else(|| GraphError::NodeNotFound {
                    id: listener_id.clone(),
                })?;
        let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
        collect_socket_evidence_cached(
            &obs_ctx,
            &observation_ids,
            &mut evidence,
            &mut seen_evidence,
        );
        let listener = GraphOwnedNode {
            id: listener_id.as_str().to_string(),
            label: listener_node.label().to_string(),
            edge_class: edge.class().to_string(),
            tag: None,
            observation_ids,
        };
        match edge.class() {
            EdgeClass::Observed if listener_node.kind() == NodeKind::Process => {
                process_listeners.push(listener);
            }
            EdgeClass::Inferred if listener_node.kind() == NodeKind::Service => {
                service_listeners.push(listener);
            }
            _ => {}
        }
    }

    process_listeners.sort_by_key(|n| n.id.clone());
    service_listeners.sort_by_key(|n| n.id.clone());
    process_callers.sort_by_key(|n| n.id.clone());
    service_callers.sort_by_key(|n| n.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(build(
        summary,
        process_listeners,
        service_listeners,
        process_callers,
        service_callers,
        evidence,
    ))
}

fn unix_socket_neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
    socket_endpoint_neighborhood(
        store,
        target,
        NodeKind::UnixSocket,
        GraphResult::unix_socket,
    )
}

fn port_neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
    socket_endpoint_neighborhood(store, target, NodeKind::Port, GraphResult::port)
}

fn collect_socket_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(store, observation_ids, evidence, seen, socket_evidence_line);
}

fn collect_connect_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(
        store,
        observation_ids,
        evidence,
        seen,
        connect_evidence_line,
    );
}

fn collect_connect_evidence_cached(
    obs_ctx: &EvidenceLoadContext,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    for obs_id in observation_ids {
        let Some(obs) = obs_ctx.observation(obs_id) else {
            continue;
        };
        if let Some(line) = connect_evidence_line(obs) {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                evidence.push(line);
            }
        }
    }
}

fn collect_socket_evidence_cached(
    obs_ctx: &EvidenceLoadContext,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    for obs_id in observation_ids {
        let Some(obs) = obs_ctx.observation(obs_id) else {
            continue;
        };
        if let Some(line) = socket_evidence_line(obs) {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                evidence.push(line);
            }
        }
    }
}

fn collect_dependency_connection_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(
        store,
        observation_ids,
        evidence,
        seen,
        dependency_connection_evidence_line,
    );
}

const CONNECT_SERVICE_RELATIONSHIP: &str =
    "process connection observed; service connection inferred from process ownership";
const CONNECT_DEPENDENCY_RELATIONSHIP: &str =
    "process connection observed; service dependency inferred from listener port match";
const CONNECT_DEPENDENCY_UNIX_RELATIONSHIP: &str =
    "process connection observed; service dependency inferred from listener path match";

fn collect_config_file_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(
        store,
        observation_ids,
        evidence,
        seen,
        config_file_evidence_line,
    );
}

fn config_file_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    let path = obs.metadata().get("path").and_then(|v| v.as_str())?;
    let service = obs.metadata().get("service").and_then(|v| v.as_str())?;
    let source_key = obs
        .metadata()
        .get("source")
        .and_then(|v| v.as_str())
        .unwrap_or("systemd_unit_file");
    let source = match source_key {
        "systemd_drop_in" => twin_collectors::ConfigFileSource::SystemdDropIn,
        "known_service_config_path" => twin_collectors::ConfigFileSource::KnownServiceConfigPath,
        _ => twin_collectors::ConfigFileSource::SystemdUnitFile,
    };
    let statement = super::scan_config_files::config_file_evidence_statement(source, path, service);
    let strength = match obs.confidence_hint() {
        twin_observation::ConfidenceHint::High => "strong",
        twin_observation::ConfidenceHint::Moderate => "moderate",
        twin_observation::ConfidenceHint::Low => "weak",
    };
    Some(GraphEvidenceLine {
        source: obs.source().to_string(),
        statement,
        strength: strength.to_string(),
        relationship: "service configured by file".to_string(),
    })
}

fn collect_systemd_unit_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(
        store,
        observation_ids,
        evidence,
        seen,
        systemd_unit_evidence_line,
    );
}

fn dependency_tag(class: EdgeClass) -> &'static str {
    if class == EdgeClass::Observed {
        "declared"
    } else {
        "runtime"
    }
}

fn systemd_unit_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    let line = graph_systemd_dep_line(obs)?;
    Some(GraphEvidenceLine {
        source: line.source,
        statement: line.statement,
        strength: line.strength,
        relationship: "configured systemd unit dependency".to_string(),
    })
}

fn connect_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    connection_evidence_line(
        obs,
        CONNECT_SERVICE_RELATIONSHIP,
        CONNECT_SERVICE_RELATIONSHIP,
    )
}

fn dependency_connection_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    connection_evidence_line(
        obs,
        CONNECT_DEPENDENCY_RELATIONSHIP,
        CONNECT_DEPENDENCY_UNIX_RELATIONSHIP,
    )
}

fn connection_evidence_line(
    obs: &Observation,
    tcp_relationship: &str,
    unix_relationship: &str,
) -> Option<GraphEvidenceLine> {
    if let Some((raw_ref, inode, local_ip, local_port, remote_ip, remote_port, fd)) =
        tcp_connection_evidence_fields(obs)
    {
        return Some(GraphEvidenceLine {
            source: raw_ref,
            statement: format!(
                "inode {inode} established from {local_ip}:{local_port} to {remote_ip}:{remote_port} joined with {fd}"
            ),
            strength: "high".to_string(),
            relationship: tcp_relationship.to_string(),
        });
    }
    unix_connection_evidence_fields(obs).map(|(raw_ref, inode, path, fd)| GraphEvidenceLine {
        source: raw_ref,
        statement: format!("inode {inode} connected on {path} joined with {fd}"),
        strength: "high".to_string(),
        relationship: unix_relationship.to_string(),
    })
}

fn tcp_connection_evidence_fields(
    obs: &Observation,
) -> Option<(String, String, String, String, String, String, String)> {
    if obs.kind() != ObservationKind::TcpConnectionSeen {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str().to_string();
    let inode = obs
        .metadata()
        .get("inode")
        .and_then(|v| v.as_str())?
        .to_string();
    let local_ip = obs
        .metadata()
        .get("local_ip")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let local_port = obs
        .metadata()
        .get("local_port")
        .and_then(|v| v.as_u64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let remote_ip = obs
        .metadata()
        .get("remote_ip")
        .and_then(|v| v.as_str())
        .unwrap_or("?")
        .to_string();
    let remote_port = obs
        .metadata()
        .get("remote_port")
        .and_then(|v| v.as_u64())
        .map(|n| n.to_string())
        .unwrap_or_else(|| "?".to_string());
    let fd = obs
        .metadata()
        .get("owner_fds")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown fd")
        .to_string();
    Some((
        raw_ref,
        inode,
        local_ip,
        local_port,
        remote_ip,
        remote_port,
        fd,
    ))
}

fn socket_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    let raw_ref = obs.raw_ref()?.as_str();
    let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
    let fd = obs
        .metadata()
        .get("owner_fds")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown fd");
    match obs.kind() {
        ObservationKind::TcpSocketSeen => Some(GraphEvidenceLine {
            source: raw_ref.to_string(),
            statement: format!("inode {inode} joined with {fd} -> socket:[{inode}]"),
            strength: "high".to_string(),
            relationship:
                "process listener observed; service listener inferred from owning process"
                    .to_string(),
        }),
        ObservationKind::UnixSocketSeen => {
            let path = obs.metadata().get("path").and_then(|v| v.as_str())?;
            Some(GraphEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!("inode {inode} listening on {path} joined with {fd}"),
                strength: "high".to_string(),
                relationship:
                    "process listener observed; service listener inferred from owning process"
                        .to_string(),
            })
        }
        _ => None,
    }
}

fn unix_connection_evidence_fields(obs: &Observation) -> Option<(String, String, String, String)> {
    if obs.kind() != ObservationKind::UnixConnectionSeen {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str().to_string();
    let inode = obs
        .metadata()
        .get("inode")
        .and_then(|v| v.as_str())?
        .to_string();
    let path = obs
        .metadata()
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|p| !p.is_empty())
        .unwrap_or("(no path)")
        .to_string();
    let fd = obs
        .metadata()
        .get("owner_fds")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown fd")
        .to_string();
    Some((raw_ref, inode, path, fd))
}

fn collect_cgroup_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    collect_observation_evidence(store, observation_ids, evidence, seen, cgroup_evidence_line);
}

fn collect_observation_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
    map_line: fn(&Observation) -> Option<GraphEvidenceLine>,
) {
    if observation_ids.len() > 1 {
        if let Ok(ctx) = EvidenceLoadContext::preload_observations(store, observation_ids) {
            for obs_id in observation_ids {
                let Some(obs) = ctx.observation(obs_id) else {
                    continue;
                };
                if let Some(line) = map_line(obs) {
                    let key = format!("{}|{}", line.source, line.statement);
                    if seen.insert(key) {
                        evidence.push(line);
                    }
                }
            }
            return;
        }
    }
    for obs_id in observation_ids {
        let Ok(id) = ObservationId::from_str(obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        if let Some(line) = map_line(&obs) {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                evidence.push(line);
            }
        }
    }
}

fn cgroup_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    if obs.kind() != ObservationKind::ProcessBelongsToCgroup {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str();
    let path = obs.metadata().get("cgroup_path").and_then(|v| v.as_str())?;
    Some(GraphEvidenceLine {
        source: raw_ref.to_string(),
        statement: format!("contains {path}"),
        strength: "high".to_string(),
        relationship: "service ownership inferred from systemd cgroup path".to_string(),
    })
}

fn peer_parent_edges(
    store: &Store,
    target: &NodeId,
    outgoing: bool,
    evidence_refs: &mut Vec<String>,
) -> Result<Vec<GraphEdgeSummary>, AppError> {
    let rows = if outgoing {
        store
            .list_active_edges_from(target.as_str())
            .map_err(GraphError::Store)?
    } else {
        store
            .list_active_edges_to(target.as_str())
            .map_err(GraphError::Store)?
    };

    let mut summaries = Vec::new();
    for row in rows {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        if edge.kind() != EdgeKind::ParentOf {
            continue;
        }
        let peer_id = if outgoing { edge.to() } else { edge.from() };
        let peer_label = store
            .get_node_typed(peer_id)
            .map_err(GraphError::Store)?
            .map(|n| n.label().to_string())
            .unwrap_or_else(|| peer_id.to_string());

        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(GraphError::Store)?;
        let mut observation_ids = Vec::new();
        for (obs_id, _) in &obs_links {
            observation_ids.push(obs_id.clone());
            if let Ok(id) = ObservationId::from_str(obs_id) {
                if let Ok(Some(obs)) = store.get_observation_typed(id) {
                    if let Some(line) = parent_evidence_line(&obs) {
                        evidence_refs.push(line);
                    } else if let Some(raw_ref) = obs.raw_ref() {
                        evidence_refs.push(raw_ref.as_str().to_string());
                    }
                }
            }
        }

        summaries.push(edge_summary(
            edge.id().as_str(),
            edge.kind(),
            peer_id,
            &peer_label,
            observation_ids,
        ));
    }
    Ok(summaries)
}

fn parent_evidence_line(obs: &Observation) -> Option<String> {
    if obs.kind() != ObservationKind::ProcessParentSeen {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str();
    let ppid = obs
        .metadata()
        .get("ppid")
        .and_then(|v| v.as_u64())
        .map(|n| n.to_string())?;
    Some(format!("{raw_ref} ppid={ppid}"))
}
