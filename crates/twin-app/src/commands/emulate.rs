use std::convert::TryFrom;

use twin_core::{EdgeKind, EdgeState, GraphEdge, NodeId, NodeKind};
use twin_emulate::{emulate_restart_service, EmulationNode, RestartServiceInput};
use twin_store::Store;

use crate::commands::impact::load_typed_service_dependents;
use crate::commands::resolve_service::{resolve_service_target, ServiceNotFoundContext};
use crate::commands::service_dependents::{impact_unknown_to_emulation, TypedServiceDependent};
use crate::error::{AppError, EmulateError};
use crate::model::EmulationResult;
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::EmulateRequest;

pub fn run_home(request: EmulateRequest) -> Result<EmulationResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    emulate_at(&paths.layout, &request, &paths.db_path)
}

pub fn run(layout: &TwinLayout, request: &EmulateRequest) -> Result<EmulationResult, AppError> {
    emulate_at(layout, request, &layout.db_file())
}

fn emulate_at(
    layout: &TwinLayout,
    request: &EmulateRequest,
    db_path: &std::path::Path,
) -> Result<EmulationResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(EmulateError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(EmulateError::StoreOpen)?;
    if !store.is_initialized().map_err(EmulateError::Store)? {
        return Err(EmulateError::DatabaseNotInitialized.into());
    }

    let target = resolve_target(&store, request)?;
    let node = store
        .get_node_typed(&target)
        .map_err(EmulateError::Store)?
        .ok_or_else(|| EmulateError::NodeNotFound { id: target.clone() })?;

    if node.kind() != NodeKind::Service {
        return Err(EmulateError::UnsupportedTarget {
            kind: node.kind().to_string(),
        }
        .into());
    }

    restart_service_emulate(&store, &target, node.label())
}

fn resolve_target(store: &Store, request: &EmulateRequest) -> Result<NodeId, AppError> {
    if let Some(target) = &request.target {
        if store
            .get_node_typed(target)
            .map_err(EmulateError::Store)?
            .is_some()
        {
            return Ok(target.clone());
        }
        return Err(EmulateError::NodeNotFound { id: target.clone() }.into());
    }
    let Some(query) = &request.target_query else {
        return Err(EmulateError::MissingTarget.into());
    };
    resolve_service_target(store, query, ServiceNotFoundContext::Emulate)
}

fn restart_service_emulate(
    store: &Store,
    target: &NodeId,
    label: &str,
) -> Result<EmulationResult, AppError> {
    let analysis = load_typed_service_dependents(store, target, label)?;
    let unavailable_nodes = load_listening_sockets(store, target)?;
    let input = RestartServiceInput {
        target: EmulationNode {
            id: target.clone(),
            label: label.to_string(),
        },
        unavailable_nodes,
        runtime_dependents: analysis
            .runtime
            .iter()
            .map(TypedServiceDependent::to_emulation_dependent)
            .collect(),
        configured_dependents: analysis
            .configured
            .iter()
            .map(TypedServiceDependent::to_emulation_dependent)
            .collect(),
        unknowns: analysis
            .unknowns
            .iter()
            .map(impact_unknown_to_emulation)
            .collect(),
    };
    let report = emulate_restart_service(input);
    Ok(EmulationResult::from_domain(report, analysis.unknowns))
}

fn load_listening_sockets(store: &Store, target: &NodeId) -> Result<Vec<EmulationNode>, AppError> {
    let mut nodes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for row in store
        .list_edges_from(target.as_str())
        .map_err(EmulateError::Store)?
    {
        let edge = GraphEdge::try_from(&row).map_err(EmulateError::Store)?;
        if edge.state() != EdgeState::Active || edge.kind() != EdgeKind::ListensOn {
            continue;
        }
        let socket_id = edge.to().clone();
        let kind = socket_id.kind();
        if !matches!(kind, Some(NodeKind::Port | NodeKind::UnixSocket)) {
            continue;
        }
        if !seen.insert(socket_id.as_str().to_string()) {
            continue;
        }
        let socket_node = store
            .get_node_typed(&socket_id)
            .map_err(EmulateError::Store)?
            .ok_or_else(|| EmulateError::NodeNotFound {
                id: socket_id.clone(),
            })?;
        nodes.push(EmulationNode {
            id: socket_id,
            label: socket_node.label().to_string(),
        });
    }
    nodes.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));
    Ok(nodes)
}
