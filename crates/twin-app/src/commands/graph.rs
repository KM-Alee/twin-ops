use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{EdgeKind, GraphEdge, NodeId, NodeKind, ObservationId};
use twin_observation::{Observation, ObservationKind};
use twin_store::Store;

use crate::error::{AppError, GraphError};
use crate::model::GraphResult;
use crate::model::{edge_summary, node_summary, GraphEdgeSummary, GraphParentEdge};
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

    if let Some(target) = &request.target {
        return neighborhood(&store, target);
    }

    let Some(kind) = request.kind else {
        return Err(GraphError::MissingQuery.into());
    };
    if kind != NodeKind::Process {
        return Err(AppError::UnsupportedGraphKind {
            kind: kind.to_string(),
        });
    }
    list_by_kind(&store, kind)
}

fn list_by_kind(store: &Store, kind: NodeKind) -> Result<GraphResult, AppError> {
    let nodes = store
        .list_nodes_by_kind_typed(kind)
        .map_err(GraphError::Store)?;
    let summaries: Vec<_> = nodes
        .iter()
        .map(|n| node_summary(n.id(), n.label()))
        .collect();
    let labels: std::collections::HashMap<String, String> = summaries
        .iter()
        .map(|n| (n.id.clone(), n.label.clone()))
        .collect();
    let node_ids: std::collections::HashSet<String> =
        summaries.iter().map(|n| n.id.clone()).collect();

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

fn neighborhood(store: &Store, target: &NodeId) -> Result<GraphResult, AppError> {
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
    let outgoing = peer_edges(store, target, true, &mut evidence_refs)?;
    let incoming = peer_edges(store, target, false, &mut evidence_refs)?;
    evidence_refs.sort();
    evidence_refs.dedup();

    Ok(GraphResult::node(
        summary,
        outgoing,
        incoming,
        evidence_refs,
    ))
}

fn peer_edges(
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
