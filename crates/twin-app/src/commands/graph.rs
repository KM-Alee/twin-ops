use std::collections::HashSet;
use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{EdgeClass, EdgeKind, GraphEdge, NodeId, NodeKind, ObservationId};
use twin_observation::{Observation, ObservationKind};
use twin_store::Store;

use crate::error::{AppError, GraphError};
use crate::model::{edge_summary, node_summary, GraphEvidenceLine, GraphOwnedNode, GraphResult};
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
        let target = resolve_service_target(&store, query)?;
        return service_neighborhood(&store, &target);
    }

    if let Some(target) = &request.target {
        return match target.kind() {
            Some(NodeKind::Service) => service_neighborhood(&store, target),
            Some(NodeKind::Process) => process_neighborhood(&store, target),
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

fn resolve_service_target(store: &Store, query: &str) -> Result<NodeId, AppError> {
    if let Ok(id) = NodeId::from_str(query) {
        if store
            .get_node_typed(&id)
            .map_err(GraphError::Store)?
            .is_some()
        {
            return Ok(id);
        }
        if id.kind() == Some(NodeKind::Service) {
            return Err(GraphError::NodeNotFound { id }.into());
        }
    }

    let unit = if query.ends_with(".service") {
        query.to_string()
    } else {
        format!("{query}.service")
    };
    let exact = NodeId::service(&unit);
    if store
        .get_node_typed(&exact)
        .map_err(GraphError::Store)?
        .is_some()
    {
        return Ok(exact);
    }

    let services = store
        .list_nodes_by_kind_typed(NodeKind::Service)
        .map_err(GraphError::Store)?;
    let needle = query.to_ascii_lowercase();
    let matches: Vec<NodeId> = services
        .iter()
        .filter(|node| {
            let id = node.id().as_str().to_ascii_lowercase();
            let label = node.label().to_ascii_lowercase();
            id.contains(&needle) || label.contains(&needle)
        })
        .map(|n| n.id().clone())
        .collect();

    match matches.len() {
        0 => Err(GraphError::ServiceNotFound {
            query: query.to_string(),
            detail: service_not_found_detail(services.len()),
        }
        .into()),
        1 => Ok(matches[0].clone()),
        _ => {
            let candidates: Vec<String> = matches.iter().map(|id| id.to_string()).collect();
            Err(GraphError::AmbiguousService {
                query: query.to_string(),
                candidates: candidates.join(", "),
            }
            .into())
        }
    }
}

fn list_by_kind(store: &Store, kind: NodeKind) -> Result<GraphResult, AppError> {
    let nodes = store
        .list_nodes_by_kind_typed(kind)
        .map_err(GraphError::Store)?;
    let summaries: Vec<_> = nodes
        .iter()
        .map(|n| node_summary(n.id(), n.label()))
        .collect();

    if kind == NodeKind::Service {
        return Ok(GraphResult::list(kind, summaries, Vec::new()));
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
        .list_edges_by_kind(&EdgeKind::ParentOf.to_string())
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
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    let outgoing = store
        .list_edges_from(target.as_str())
        .map_err(GraphError::Store)?;
    for row in outgoing {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
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
            observation_ids,
        };
        match peer_node.kind() {
            NodeKind::Process => owned_processes.push(owned),
            NodeKind::Cgroup => owned_cgroups.push(owned),
            _ => {}
        }
    }

    owned_processes.sort_by_key(|n| n.id.clone());
    owned_cgroups.sort_by_key(|n| n.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(GraphResult::service(
        summary,
        owned_processes,
        owned_cgroups,
        evidence,
    ))
}

fn collect_cgroup_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<GraphEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    for obs_id in observation_ids {
        let Ok(id) = ObservationId::from_str(obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        if let Some(line) = cgroup_evidence_line(&obs) {
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
            .list_edges_from(target.as_str())
            .map_err(GraphError::Store)?
    } else {
        store
            .list_edges_to(target.as_str())
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
