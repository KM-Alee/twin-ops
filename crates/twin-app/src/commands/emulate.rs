use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{EdgeKind, EdgeState, GraphEdge, NodeId, NodeKind, ObservationId, UnknownKind};
use twin_emulate::{
    emulate_delete_file, emulate_restart_service, DeleteFileInput, EmulationConfiguredService,
    EmulationEvidenceLine, EmulationNode, RestartServiceInput,
};
use twin_store::Store;

use crate::commands::impact::load_typed_service_dependents;
use crate::commands::resolve_service::{resolve_service_target, ServiceNotFoundContext};
use crate::commands::scan_config_files::{config_file_evidence_statement, host_path_for_discovery};
use crate::commands::scan_quality::assess_scan_quality;
use crate::commands::service_dependents::{impact_unknown_to_emulation, TypedServiceDependent};
use crate::error::{AppError, EmulateError};
use crate::model::{EmulationResult, ImpactUnknown};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::EmulateActionRequest;
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

    match &request.action {
        EmulateActionRequest::Restart {
            target,
            target_query,
        } => {
            let resolved = resolve_restart_target(&store, target, target_query)?;
            let node = store
                .get_node_typed(&resolved)
                .map_err(EmulateError::Store)?
                .ok_or_else(|| EmulateError::NodeNotFound {
                    id: resolved.clone(),
                })?;
            if node.kind() != NodeKind::Service {
                return Err(EmulateError::UnsupportedTarget {
                    kind: node.kind().to_string(),
                }
                .into());
            }
            restart_service_emulate(&store, &resolved, node.label())
        }
        EmulateActionRequest::DeleteFile { path } => delete_file_emulate(&store, path),
    }
}

fn resolve_restart_target(
    store: &Store,
    target: &Option<NodeId>,
    target_query: &Option<String>,
) -> Result<NodeId, AppError> {
    if let Some(target) = target {
        if store
            .get_node_typed(target)
            .map_err(EmulateError::Store)?
            .is_some()
        {
            return Ok(target.clone());
        }
        return Err(EmulateError::NodeNotFound { id: target.clone() }.into());
    }
    let Some(query) = target_query else {
        return Err(EmulateError::MissingTarget.into());
    };
    resolve_service_target(store, query, ServiceNotFoundContext::Emulate)
}

fn resolve_delete_file_target(raw: &str) -> Result<NodeId, AppError> {
    if raw.starts_with("file:") {
        let id = NodeId::from_str(raw).map_err(|source| EmulateError::InvalidDeleteTarget {
            value: raw.to_string(),
            reason: source.to_string(),
        })?;
        if id.kind() != Some(NodeKind::File) {
            return Err(EmulateError::InvalidDeleteTarget {
                value: raw.to_string(),
                reason: "not a file node id".to_string(),
            }
            .into());
        }
        return Ok(id);
    }
    if let Ok(id) = NodeId::from_str(raw) {
        if id.kind() == Some(NodeKind::Service) {
            return Err(EmulateError::InvalidDeleteTarget {
                value: raw.to_string(),
                reason: "delete emulation requires a file, not a service".to_string(),
            }
            .into());
        }
        if id.kind() == Some(NodeKind::File) {
            return Ok(id);
        }
        return Err(EmulateError::InvalidDeleteTarget {
            value: raw.to_string(),
            reason: format!("unsupported node kind {:?}", id.kind()),
        }
        .into());
    }
    if !raw.starts_with('/') {
        return Err(EmulateError::RelativeDeletePath {
            value: raw.to_string(),
        }
        .into());
    }
    Ok(NodeId::file(raw))
}

fn delete_file_emulate(store: &Store, raw_path: &str) -> Result<EmulationResult, AppError> {
    let target = resolve_delete_file_target(raw_path)?;
    let file_in_graph = store
        .get_node_typed(&target)
        .map_err(EmulateError::Store)?
        .is_some();
    let path_label = target
        .as_str()
        .strip_prefix("file:")
        .unwrap_or(target.as_str())
        .to_string();
    let mapped = host_path_for_discovery(&path_label);
    let file_exists = mapped.is_file();

    let mut configured_services = Vec::new();
    let mut evidence = Vec::new();
    let mut unknowns: Vec<ImpactUnknown> = Vec::new();

    for row in store
        .list_edges_to(target.as_str())
        .map_err(EmulateError::Store)?
    {
        let edge = GraphEdge::try_from(&row).map_err(EmulateError::Store)?;
        if edge.state() != EdgeState::Active || edge.kind() != EdgeKind::ConfiguredBy {
            continue;
        }
        let service_id = edge.from().clone();
        let service_node = store
            .get_node_typed(&service_id)
            .map_err(EmulateError::Store)?
            .ok_or_else(|| EmulateError::NodeNotFound {
                id: service_id.clone(),
            })?;
        let service_evidence = load_configured_by_evidence(store, &edge)?;
        evidence.extend(service_evidence.iter().cloned());
        configured_services.push(EmulationConfiguredService {
            id: service_id,
            label: service_node.label().to_string(),
            file_id: target.clone(),
            evidence: service_evidence,
        });
    }
    configured_services.sort_by(|a, b| a.id.as_str().cmp(b.id.as_str()));

    let assessment = assess_scan_quality(store)?;
    if let Some(note) = super::scan_quality::scan_health_note(&assessment) {
        unknowns.push(ImpactUnknown {
            kind: UnknownKind::ScanHealth.as_str().to_string(),
            detail: note,
            source: Some("latest scan".to_string()),
            weakens_evidence: false,
        });
    }
    if !file_in_graph {
        unknowns.push(ImpactUnknown {
            kind: UnknownKind::MissingEvidence.as_str().to_string(),
            detail: "target file is not in the graph; run `twin scan` first".to_string(),
            source: None,
            weakens_evidence: true,
        });
    }

    let emulation_unknowns: Vec<_> = unknowns.iter().map(impact_unknown_to_emulation).collect();
    let input = DeleteFileInput {
        target: EmulationNode {
            id: target.clone(),
            label: path_label,
        },
        configured_services,
        evidence,
        unknowns: emulation_unknowns,
        file_in_graph,
        file_exists,
    };
    let report = emulate_delete_file(input);
    Ok(EmulationResult::from_domain(report, unknowns))
}

fn load_configured_by_evidence(
    store: &Store,
    edge: &GraphEdge,
) -> Result<Vec<EmulationEvidenceLine>, AppError> {
    let obs_links = store
        .list_observations_for_edge(edge.id().as_str())
        .map_err(EmulateError::Store)?;
    let mut lines = Vec::new();
    for (obs_id, _) in obs_links {
        let Ok(obs_id) = ObservationId::from_str(&obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(obs_id) else {
            continue;
        };
        let path = obs
            .metadata()
            .get("path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let service = obs
            .metadata()
            .get("service")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let source_key = obs
            .metadata()
            .get("source")
            .and_then(|v| v.as_str())
            .unwrap_or("systemd_unit_file");
        let source = match source_key {
            "systemd_drop_in" => twin_collectors::ConfigFileSource::SystemdDropIn,
            "known_service_config_path" => {
                twin_collectors::ConfigFileSource::KnownServiceConfigPath
            }
            _ => twin_collectors::ConfigFileSource::SystemdUnitFile,
        };
        let statement = config_file_evidence_statement(source, path, service);
        let strength_score = match obs.confidence_hint() {
            twin_observation::ConfidenceHint::High => 90,
            twin_observation::ConfidenceHint::Moderate => 70,
            twin_observation::ConfidenceHint::Low => 40,
        };
        lines.push(EmulationEvidenceLine {
            source: obs.source().to_string(),
            statement,
            relationship: "configured_by".to_string(),
            strength_score,
        });
    }
    if lines.is_empty() {
        let path = serde_json::from_str::<serde_json::Value>(edge.metadata().as_str())
            .ok()
            .and_then(|v| v.get("path").and_then(|p| p.as_str()).map(str::to_string))
            .unwrap_or_default();
        lines.push(EmulationEvidenceLine {
            source: "graph".to_string(),
            statement: format!("service configured_by file {path}"),
            relationship: "configured_by".to_string(),
            strength_score: 50,
        });
    }
    Ok(lines)
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
