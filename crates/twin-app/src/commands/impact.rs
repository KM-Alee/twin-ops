use std::collections::HashSet;
use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{EdgeClass, EdgeKind, GraphEdge, NodeId, NodeKind, ObservationId};
use twin_observation::{Observation, ObservationKind};
use twin_store::Store;

use crate::error::{AppError, ImpactError};
use crate::model::{GraphEvidenceLine, ImpactDependent, ImpactEvidenceLine, ImpactResult};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::ImpactRequest;

pub fn run_home(request: ImpactRequest) -> Result<ImpactResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    impact_at(&paths.layout, &request, &paths.db_path)
}

pub fn run(layout: &TwinLayout, request: &ImpactRequest) -> Result<ImpactResult, AppError> {
    impact_at(layout, request, &layout.db_file())
}

fn impact_at(
    layout: &TwinLayout,
    request: &ImpactRequest,
    db_path: &std::path::Path,
) -> Result<ImpactResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(ImpactError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(ImpactError::StoreOpen)?;
    if !store.is_initialized().map_err(ImpactError::Store)? {
        return Err(ImpactError::DatabaseNotInitialized.into());
    }

    let target = resolve_target(&store, request)?;
    let node = store
        .get_node_typed(&target)
        .map_err(ImpactError::Store)?
        .ok_or_else(|| ImpactError::NodeNotFound { id: target.clone() })?;

    match node.kind() {
        NodeKind::Service => service_impact(&store, &target, node.label()),
        NodeKind::Port => port_impact(&store, &target, node.label()),
        kind => Err(ImpactError::UnsupportedTarget {
            kind: kind.to_string(),
        }
        .into()),
    }
}

fn resolve_target(store: &Store, request: &ImpactRequest) -> Result<NodeId, AppError> {
    if let Some(target) = &request.target {
        if store
            .get_node_typed(target)
            .map_err(ImpactError::Store)?
            .is_some()
        {
            return Ok(target.clone());
        }
        if target.kind() == Some(NodeKind::Service) {
            return Err(ImpactError::NodeNotFound { id: target.clone() }.into());
        }
        return Err(ImpactError::NodeNotFound { id: target.clone() }.into());
    }
    let Some(query) = &request.target_query else {
        return Err(ImpactError::MissingTarget.into());
    };
    resolve_service_target(store, query)
}

fn resolve_service_target(store: &Store, query: &str) -> Result<NodeId, AppError> {
    if let Ok(id) = NodeId::from_str(query) {
        if store
            .get_node_typed(&id)
            .map_err(ImpactError::Store)?
            .is_some()
        {
            return Ok(id);
        }
        if id.kind() == Some(NodeKind::Service) {
            return Err(ImpactError::NodeNotFound { id }.into());
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
        .map_err(ImpactError::Store)?
        .is_some()
    {
        return Ok(exact);
    }

    let services = store
        .list_nodes_by_kind_typed(NodeKind::Service)
        .map_err(ImpactError::Store)?;
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
        0 => Err(ImpactError::ServiceNotFound {
            query: query.to_string(),
        }
        .into()),
        1 => Ok(matches[0].clone()),
        _ => {
            let candidates: Vec<String> = matches.iter().map(|id| id.to_string()).collect();
            Err(ImpactError::AmbiguousService {
                query: query.to_string(),
                candidates: candidates.join(", "),
            }
            .into())
        }
    }
}

fn service_impact(store: &Store, target: &NodeId, label: &str) -> Result<ImpactResult, AppError> {
    let mut direct_dependents = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    let incoming = store
        .list_edges_to(target.as_str())
        .map_err(ImpactError::Store)?;
    for row in incoming {
        let edge = GraphEdge::try_from(&row).map_err(ImpactError::Store)?;
        if edge.kind() != EdgeKind::DependsOn || edge.class() != EdgeClass::Inferred {
            continue;
        }
        let dependent_id = edge.from();
        let dependent_node = store
            .get_node_typed(dependent_id)
            .map_err(ImpactError::Store)?
            .ok_or_else(|| ImpactError::NodeNotFound {
                id: dependent_id.clone(),
            })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(ImpactError::Store)?;
        let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
        collect_impact_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
        direct_dependents.push(ImpactDependent {
            id: dependent_id.as_str().to_string(),
            label: dependent_node.label().to_string(),
            relationship: "depends_on".to_string(),
            edge_class: edge.class().to_string(),
            observation_ids,
        });
    }

    direct_dependents.sort_by_key(|d| d.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(ImpactResult {
        target: target.to_string(),
        target_label: label.to_string(),
        direct_dependents,
        evidence,
        unknowns: Vec::new(),
    })
}

fn port_impact(store: &Store, target: &NodeId, label: &str) -> Result<ImpactResult, AppError> {
    let mut direct_dependents = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();

    let incoming = store
        .list_edges_to(target.as_str())
        .map_err(ImpactError::Store)?;
    for row in incoming {
        let edge = GraphEdge::try_from(&row).map_err(ImpactError::Store)?;
        if edge.kind() != EdgeKind::ConnectsTo {
            continue;
        }
        let caller_id = edge.from();
        let caller_node = store
            .get_node_typed(caller_id)
            .map_err(ImpactError::Store)?
            .ok_or_else(|| ImpactError::NodeNotFound {
                id: caller_id.clone(),
            })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(ImpactError::Store)?;
        let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
        collect_impact_evidence(store, &observation_ids, &mut evidence, &mut seen_evidence);
        direct_dependents.push(ImpactDependent {
            id: caller_id.as_str().to_string(),
            label: caller_node.label().to_string(),
            relationship: "connects_to".to_string(),
            edge_class: edge.class().to_string(),
            observation_ids,
        });
    }

    direct_dependents.sort_by_key(|d| d.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));

    Ok(ImpactResult {
        target: target.to_string(),
        target_label: label.to_string(),
        direct_dependents,
        evidence,
        unknowns: Vec::new(),
    })
}

fn collect_impact_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<ImpactEvidenceLine>,
    seen: &mut HashSet<String>,
) {
    for obs_id in observation_ids {
        let Ok(id) = ObservationId::from_str(obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        if let Some(line) = impact_evidence_line(&obs) {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                evidence.push(line);
            }
        }
    }
}

fn impact_evidence_line(obs: &Observation) -> Option<ImpactEvidenceLine> {
    match obs.kind() {
        ObservationKind::TcpConnectionSeen => connection_evidence_line(obs),
        ObservationKind::TcpSocketSeen => {
            let line = socket_evidence_line(obs)?;
            Some(ImpactEvidenceLine {
                source: line.source,
                statement: line.statement,
                relationship: line.relationship,
            })
        }
        _ => None,
    }
}

fn socket_evidence_line(obs: &Observation) -> Option<GraphEvidenceLine> {
    if obs.kind() != ObservationKind::TcpSocketSeen {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str();
    let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
    let fd = obs
        .metadata()
        .get("owner_fds")
        .and_then(|v| v.as_array())
        .and_then(|arr| arr.first())
        .and_then(|v| v.as_str())
        .unwrap_or("unknown fd");
    Some(GraphEvidenceLine {
        source: raw_ref.to_string(),
        statement: format!("inode {inode} joined with {fd} -> socket:[{inode}]"),
        strength: "high".to_string(),
        relationship: "process listener observed; service listener inferred from owning process"
            .to_string(),
    })
}

fn connection_evidence_line(obs: &Observation) -> Option<ImpactEvidenceLine> {
    if obs.kind() != ObservationKind::TcpConnectionSeen {
        return None;
    }
    let raw_ref = obs.raw_ref()?.as_str();
    let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
    let local_ip = obs
        .metadata()
        .get("local_ip")
        .and_then(|v| v.as_str())
        .unwrap_or("?");
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
        .unwrap_or("?");
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
        .unwrap_or("unknown fd");
    Some(ImpactEvidenceLine {
        source: raw_ref.to_string(),
        statement: format!(
            "inode {inode} established from {local_ip}:{local_port} to {remote_ip}:{remote_port} joined with {fd}"
        ),
        relationship:
            "process connection observed; service dependency inferred from listener match"
                .to_string(),
    })
}
