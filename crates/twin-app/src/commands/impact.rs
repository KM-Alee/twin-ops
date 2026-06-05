use std::collections::HashSet;
use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{
    EdgeClass, EdgeKind, EdgeState, EvidenceStrength, GraphEdge, NodeId, NodeKind, ObservationId,
    RiskLevel,
};
use twin_observation::{Observation, ObservationKind};
use twin_store::Store;

use crate::commands::evidence::{
    is_runtime_active_metadata, systemd_dep_strength_label, systemd_impact_statement,
};
use crate::commands::resolve_service::{resolve_service_target, ServiceNotFoundContext};
use crate::commands::scan_quality::{assess_scan_quality, scan_health_note};
use crate::error::{AppError, ImpactError};
use crate::model::{
    GraphOwnedNode, ImpactDependent, ImpactEvidenceLine, ImpactNodeSummary, ImpactPathStep,
    ImpactResult, ImpactUnknown, RiskAssessment,
};
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
        NodeKind::Port | NodeKind::UnixSocket => {
            port_impact(&store, &target, node.label(), node.kind())
        }
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
        return Err(ImpactError::NodeNotFound { id: target.clone() }.into());
    }
    let Some(query) = &request.target_query else {
        return Err(ImpactError::MissingTarget.into());
    };
    resolve_service_target(store, query, ServiceNotFoundContext::Impact)
}

fn service_impact(store: &Store, target: &NodeId, label: &str) -> Result<ImpactResult, AppError> {
    let target_summary = node_summary(target, label);
    let mut direct_dependents = Vec::new();
    let mut configured_dependents = Vec::new();
    let mut seen_evidence = HashSet::new();
    let tcp_cache = unmapped_tcp_observations(store)?;
    let unix_cache = unmapped_unix_observations(store)?;
    let mut unknowns = target_unknowns_for_service(store, target, &tcp_cache, &unix_cache)?;

    let incoming = store
        .list_edges_to(target.as_str())
        .map_err(ImpactError::Store)?;
    for row in incoming {
        let edge = GraphEdge::try_from(&row).map_err(ImpactError::Store)?;
        if !is_active_edge(&edge) {
            continue;
        }
        if edge.kind() != EdgeKind::DependsOn {
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
        if observation_ids.is_empty() {
            unknowns.push(missing_evidence_unknown(edge.id().as_str()));
        }
        let mut dependent_evidence = Vec::new();
        let relationship = if edge.class() == EdgeClass::Observed {
            "Configured: systemd unit file dependency"
        } else {
            "Inferred: service dependency inferred from active connection and listener match"
        };
        collect_edge_evidence(
            store,
            &observation_ids,
            &mut dependent_evidence,
            &mut seen_evidence,
            relationship,
        );
        let reason = service_dependent_reason(&edge, &dependent_evidence, target, label);
        let impact_kind = classify_dependent_impact_kind(&edge, dependent_node.metadata().as_str());
        let dependent = build_dependent(DependentInput {
            dependent_id,
            dependent_label: dependent_node.label(),
            edge_kind: EdgeKind::DependsOn,
            edge_class: edge.class(),
            impact_kind,
            target: &target_summary,
            edge_id: edge.id().as_str(),
            reason,
            evidence: dependent_evidence,
            observation_ids,
        });
        if impact_kind == "runtime" {
            direct_dependents.push(dependent);
        } else {
            configured_dependents.push(dependent);
        }
    }

    append_scan_health_note(store, &mut unknowns)?;

    direct_dependents.sort_by_key(|d| d.id.clone());
    configured_dependents.sort_by_key(|d| d.id.clone());
    unknowns.sort_by(|a, b| a.kind.cmp(&b.kind));

    let risk = score_risk(
        NodeKind::Service,
        &direct_dependents,
        &configured_dependents,
        &unknowns,
    );
    let evidence_strength = score_evidence(&direct_dependents, &[], &unknowns);

    Ok(ImpactResult {
        target: target.to_string(),
        target_label: label.to_string(),
        risk,
        evidence_strength: evidence_strength.into(),
        direct_dependents,
        configured_dependents,
        listener_owners: Vec::new(),
        evidence: Vec::new(),
        unknowns,
    })
}

fn classify_dependent_impact_kind(edge: &GraphEdge, dependent_metadata: &str) -> &'static str {
    if edge.class() == EdgeClass::Inferred {
        return "runtime";
    }
    if is_runtime_active_metadata(dependent_metadata) {
        "runtime"
    } else {
        "configured"
    }
}

fn port_impact(
    store: &Store,
    target: &NodeId,
    label: &str,
    target_kind: NodeKind,
) -> Result<ImpactResult, AppError> {
    let target_summary = node_summary(target, label);
    let mut direct_dependents = Vec::new();
    let mut listener_owners = Vec::new();
    let mut evidence = Vec::new();
    let mut seen_evidence = HashSet::new();
    let tcp_cache = unmapped_tcp_observations(store)?;
    let unix_cache = unmapped_unix_observations(store)?;
    let mut unknowns = target_unknowns_for_socket(target, target_kind, &tcp_cache, &unix_cache)?;

    let incoming = store
        .list_edges_to(target.as_str())
        .map_err(ImpactError::Store)?;
    for row in incoming {
        let edge = GraphEdge::try_from(&row).map_err(ImpactError::Store)?;
        if !is_active_edge(&edge) {
            continue;
        }
        if edge.kind() == EdgeKind::ConnectsTo {
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
            if observation_ids.is_empty() {
                unknowns.push(missing_evidence_unknown(edge.id().as_str()));
            }
            let mut dependent_evidence = Vec::new();
            collect_edge_evidence(
                store,
                &observation_ids,
                &mut dependent_evidence,
                &mut seen_evidence,
                "Inferred: service connection inferred from process ownership",
            );
            let reason = format!("active connection to {label}");
            direct_dependents.push(build_dependent(DependentInput {
                dependent_id: caller_id,
                dependent_label: caller_node.label(),
                edge_kind: EdgeKind::ConnectsTo,
                edge_class: edge.class(),
                impact_kind: "runtime",
                target: &target_summary,
                edge_id: edge.id().as_str(),
                reason,
                evidence: dependent_evidence,
                observation_ids,
            }));
            continue;
        }
        if edge.kind() != EdgeKind::ListensOn {
            continue;
        }
        let listener_id = edge.from();
        let listener_node = store
            .get_node_typed(listener_id)
            .map_err(ImpactError::Store)?
            .ok_or_else(|| ImpactError::NodeNotFound {
                id: listener_id.clone(),
            })?;
        let obs_links = store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(ImpactError::Store)?;
        let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
        collect_edge_evidence(
            store,
            &observation_ids,
            &mut evidence,
            &mut seen_evidence,
            "Observed: listener socket observed; service listener inferred from owning process",
        );
        match (edge.class(), listener_node.kind()) {
            (EdgeClass::Observed, NodeKind::Process) | (EdgeClass::Inferred, NodeKind::Service) => {
                listener_owners.push(GraphOwnedNode {
                    id: listener_id.as_str().to_string(),
                    label: listener_node.label().to_string(),
                    edge_class: edge.class().to_string(),
                    tag: None,
                    observation_ids,
                });
            }
            _ => {}
        }
    }

    append_scan_health_note(store, &mut unknowns)?;

    direct_dependents.sort_by_key(|d| d.id.clone());
    listener_owners.sort_by_key(|n| n.id.clone());
    evidence.sort_by(|a, b| a.source.cmp(&b.source));
    unknowns.sort_by(|a, b| a.kind.cmp(&b.kind));

    let risk = score_risk(target_kind, &direct_dependents, &[], &unknowns);
    let evidence_strength = score_evidence(&direct_dependents, &evidence, &unknowns);

    Ok(ImpactResult {
        target: target.to_string(),
        target_label: label.to_string(),
        risk,
        evidence_strength: evidence_strength.into(),
        direct_dependents,
        configured_dependents: Vec::new(),
        listener_owners,
        evidence,
        unknowns,
    })
}

struct DependentInput<'a> {
    dependent_id: &'a NodeId,
    dependent_label: &'a str,
    edge_kind: EdgeKind,
    edge_class: EdgeClass,
    impact_kind: &'a str,
    target: &'a ImpactNodeSummary,
    edge_id: &'a str,
    reason: String,
    evidence: Vec<ImpactEvidenceLine>,
    observation_ids: Vec<String>,
}

fn build_dependent(input: DependentInput<'_>) -> ImpactDependent {
    let from = node_summary(input.dependent_id, input.dependent_label);
    ImpactDependent {
        id: from.id.clone(),
        label: from.label.clone(),
        relationship: input.edge_kind.to_string(),
        edge_class: input.edge_class.to_string(),
        impact_kind: input.impact_kind.to_string(),
        reason: input.reason,
        path: vec![ImpactPathStep {
            from: from.clone(),
            edge_kind: input.edge_kind.to_string(),
            edge_class: input.edge_class.to_string(),
            to: input.target.clone(),
            edge_id: input.edge_id.to_string(),
        }],
        evidence: input.evidence,
        observation_ids: input.observation_ids,
    }
}

fn node_summary(id: &NodeId, label: &str) -> ImpactNodeSummary {
    ImpactNodeSummary {
        id: id.to_string(),
        label: label.to_string(),
    }
}

fn is_active_edge(edge: &GraphEdge) -> bool {
    edge.state() == EdgeState::Active
}

fn service_dependent_reason(
    edge: &GraphEdge,
    evidence: &[ImpactEvidenceLine],
    _target: &NodeId,
    target_label: &str,
) -> String {
    if edge.class() == EdgeClass::Observed {
        for line in evidence {
            if let Some((key, to_unit)) = configured_dep_from_statement(&line.statement) {
                return format!("{key}={to_unit} in unit file");
            }
        }
        return format!("declared dependency on {target_label} in unit file");
    }
    dependent_reason(evidence, target_label)
}

fn configured_dep_from_statement(statement: &str) -> Option<(&str, &str)> {
    let rest = statement.split("Configured: ").nth(1)?;
    let key = rest.split('=').next()?;
    if !matches!(key, "Requires" | "Wants" | "BindsTo") {
        return None;
    }
    let after_key = rest.strip_prefix(key)?.strip_prefix('=')?;
    let to_unit = after_key.split_whitespace().next()?;
    Some((key, to_unit))
}

fn dependent_reason(evidence: &[ImpactEvidenceLine], target_label: &str) -> String {
    for line in evidence {
        if let Some(endpoint) = connection_endpoint_from_statement(&line.statement) {
            return format!("active connection to {endpoint}");
        }
    }
    format!("active connection to {target_label}")
}

fn connection_endpoint_from_statement(statement: &str) -> Option<String> {
    if let Some(rest) = statement.split(" connected on ").nth(1) {
        let endpoint = rest.split(" joined").next()?.trim();
        if endpoint == "(no path)" {
            return None;
        }
        return Some(endpoint.to_string());
    }
    let marker = " to ";
    let after = statement.split(marker).nth(1)?;
    let endpoint = after.split(" joined").next()?.trim();
    if endpoint.contains(':') {
        Some(endpoint.to_string())
    } else {
        None
    }
}

fn collect_edge_evidence(
    store: &Store,
    observation_ids: &[String],
    evidence: &mut Vec<ImpactEvidenceLine>,
    seen: &mut HashSet<String>,
    relationship: &str,
) {
    for obs_id in observation_ids {
        let Ok(id) = ObservationId::from_str(obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        if let Some(line) = impact_evidence_line(&obs, relationship) {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                evidence.push(line);
            }
        }
    }
}

fn impact_evidence_line(obs: &Observation, relationship: &str) -> Option<ImpactEvidenceLine> {
    let observation_id = Some(obs.id().to_string());
    let strength = evidence_strength_for_observation(obs).label().to_string();
    match obs.kind() {
        ObservationKind::TcpConnectionSeen => {
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
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!(
                    "Observed: inode {inode} established from {local_ip}:{local_port} to {remote_ip}:{remote_port}"
                ),
                relationship: relationship.to_string(),
                strength,
                observation_id,
            })
        }
        ObservationKind::TcpSocketSeen => {
            let raw_ref = obs.raw_ref()?.as_str();
            let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
            let fd = obs
                .metadata()
                .get("owner_fds")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("unknown fd");
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!("Observed: listener inode {inode} joined with {fd}"),
                relationship: relationship.to_string(),
                strength,
                observation_id,
            })
        }
        ObservationKind::UnixSocketSeen => {
            let raw_ref = obs.raw_ref()?.as_str();
            let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
            let path = obs.metadata().get("path").and_then(|v| v.as_str())?;
            let fd = obs
                .metadata()
                .get("owner_fds")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("unknown fd");
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!("Observed: listener inode {inode} on {path} joined with {fd}"),
                relationship: relationship.to_string(),
                strength,
                observation_id,
            })
        }
        ObservationKind::UnixConnectionSeen => {
            let raw_ref = obs.raw_ref()?.as_str();
            let inode = obs.metadata().get("inode").and_then(|v| v.as_str())?;
            let path = obs.metadata().get("path").and_then(|v| v.as_str())?;
            let fd = obs
                .metadata()
                .get("owner_fds")
                .and_then(|v| v.as_array())
                .and_then(|arr| arr.first())
                .and_then(|v| v.as_str())
                .unwrap_or("unknown fd");
            let path_label = if path.is_empty() { "(no path)" } else { path };
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!(
                    "Observed: inode {inode} connected on {path_label} joined with {fd}"
                ),
                relationship: relationship.to_string(),
                strength,
                observation_id,
            })
        }
        ObservationKind::ProcessBelongsToCgroup => {
            let raw_ref = obs.raw_ref()?.as_str();
            let path = obs.metadata().get("cgroup_path").and_then(|v| v.as_str())?;
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement: format!("Inferred: service ownership inferred from {path}"),
                relationship: relationship.to_string(),
                strength,
                observation_id,
            })
        }
        ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants => {
            let raw_ref = obs.raw_ref()?.as_str();
            let (statement, strength_label) = systemd_impact_statement(obs)?;
            Some(ImpactEvidenceLine {
                source: raw_ref.to_string(),
                statement,
                relationship: relationship.to_string(),
                strength: strength_label,
                observation_id,
            })
        }
        _ => None,
    }
}

fn evidence_strength_for_observation(obs: &Observation) -> EvidenceStrength {
    match obs.kind() {
        ObservationKind::TcpConnectionSeen | ObservationKind::UnixConnectionSeen => {
            EvidenceStrength::strong()
        }
        ObservationKind::TcpSocketSeen | ObservationKind::UnixSocketSeen => {
            EvidenceStrength::strong()
        }
        ObservationKind::ProcessBelongsToCgroup => EvidenceStrength::moderate(),
        ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants => {
            let key = obs
                .metadata()
                .get("key")
                .and_then(|v| v.as_str())
                .unwrap_or("Requires");
            EvidenceStrength::new(match systemd_dep_strength_label(key) {
                "strong" => 75,
                "moderate" => 45,
                _ => 20,
            })
        }
        _ => EvidenceStrength::weak(),
    }
}

fn append_scan_health_note(
    store: &Store,
    unknowns: &mut Vec<ImpactUnknown>,
) -> Result<(), AppError> {
    let assessment = assess_scan_quality(store)?;
    if let Some(note) = scan_health_note(&assessment) {
        unknowns.push(ImpactUnknown {
            kind: "scan_health".to_string(),
            detail: note,
            source: Some("latest scan".to_string()),
            weakens_evidence: false,
        });
    }
    Ok(())
}

fn unmapped_tcp_observations(store: &Store) -> Result<Vec<Observation>, AppError> {
    unmapped_socket_observations(store, "proc_net_tcp")
}

fn unmapped_unix_observations(store: &Store) -> Result<Vec<Observation>, AppError> {
    unmapped_socket_observations(store, "proc_net_unix")
}

fn unmapped_socket_observations(store: &Store, source: &str) -> Result<Vec<Observation>, AppError> {
    let rows = store
        .list_observations_by_source(source)
        .map_err(ImpactError::Store)?;
    let mut out = Vec::new();
    for row in rows {
        let Ok(obs) = Observation::try_from(&row) else {
            continue;
        };
        if obs
            .metadata()
            .get("mapped")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            continue;
        }
        out.push(obs);
    }
    Ok(out)
}

fn target_unknowns_for_service(
    store: &Store,
    target: &NodeId,
    tcp_cache: &[Observation],
    unix_cache: &[Observation],
) -> Result<Vec<ImpactUnknown>, AppError> {
    let mut unknowns = Vec::new();
    let (port_ids, unix_ids) = socket_peers_for_service(store, target)?;
    for port_id in &port_ids {
        unknowns.extend(unmapped_sockets_for_port(port_id, tcp_cache)?);
    }
    for unix_id in &unix_ids {
        unknowns.extend(unmapped_sockets_for_unix(unix_id, unix_cache)?);
    }
    Ok(unknowns)
}

fn target_unknowns_for_socket(
    target: &NodeId,
    target_kind: NodeKind,
    tcp_cache: &[Observation],
    unix_cache: &[Observation],
) -> Result<Vec<ImpactUnknown>, AppError> {
    match target_kind {
        NodeKind::Port => unmapped_sockets_for_port(target, tcp_cache),
        NodeKind::UnixSocket => unmapped_sockets_for_unix(target, unix_cache),
        _ => Ok(Vec::new()),
    }
}

fn socket_peers_for_service(
    store: &Store,
    service_id: &NodeId,
) -> Result<(Vec<NodeId>, Vec<NodeId>), AppError> {
    let mut ports = Vec::new();
    let mut unix_sockets = Vec::new();
    for row in store
        .list_edges_from(service_id.as_str())
        .map_err(ImpactError::Store)?
    {
        let Ok(edge) = GraphEdge::try_from(&row) else {
            continue;
        };
        if edge.kind() != EdgeKind::ListensOn && edge.kind() != EdgeKind::ConnectsTo {
            continue;
        }
        let peer = edge.to().clone();
        match peer.kind() {
            Some(NodeKind::Port) if !ports.contains(&peer) => ports.push(peer),
            Some(NodeKind::UnixSocket) if !unix_sockets.contains(&peer) => unix_sockets.push(peer),
            _ => {}
        }
    }
    Ok((ports, unix_sockets))
}

fn unmapped_sockets_for_port(
    port_id: &NodeId,
    tcp_cache: &[Observation],
) -> Result<Vec<ImpactUnknown>, AppError> {
    let (ip, port) = parse_port_node(port_id)?;
    let mut unknowns = Vec::new();
    let mut unmapped_active = 0usize;
    let mut unmapped_listener = 0usize;

    for obs in tcp_cache {
        if !observation_matches_port(obs, &ip, port) {
            continue;
        }
        match obs.kind() {
            ObservationKind::TcpConnectionSeen => unmapped_active += 1,
            ObservationKind::TcpSocketSeen => unmapped_listener += 1,
            _ => {}
        }
    }

    if unmapped_active > 0 {
        unknowns.push(ImpactUnknown {
            kind: "unmapped_active_sockets".to_string(),
            detail: format!(
                "{unmapped_active} active TCP sockets on {ip}:{port} could not be mapped to a process"
            ),
            source: Some(port_id.to_string()),
            weakens_evidence: true,
        });
    }
    if unmapped_listener > 0 {
        unknowns.push(ImpactUnknown {
            kind: "unmapped_listener_sockets".to_string(),
            detail: format!(
                "{unmapped_listener} listener sockets on {ip}:{port} could not be mapped to a process"
            ),
            source: Some(port_id.to_string()),
            weakens_evidence: true,
        });
    }
    Ok(unknowns)
}

fn unmapped_sockets_for_unix(
    unix_id: &NodeId,
    unix_cache: &[Observation],
) -> Result<Vec<ImpactUnknown>, AppError> {
    let path =
        unix_id
            .as_str()
            .strip_prefix("unix:")
            .ok_or_else(|| ImpactError::UnsupportedTarget {
                kind: unix_id.to_string(),
            })?;
    let mut unknowns = Vec::new();
    let mut unmapped_active = 0usize;
    let mut unmapped_listener = 0usize;

    for obs in unix_cache {
        if obs.metadata().get("path").and_then(|v| v.as_str()) != Some(path) {
            continue;
        }
        match obs.kind() {
            ObservationKind::UnixConnectionSeen => unmapped_active += 1,
            ObservationKind::UnixSocketSeen => unmapped_listener += 1,
            _ => {}
        }
    }

    if unmapped_active > 0 {
        unknowns.push(ImpactUnknown {
            kind: "unmapped_active_sockets".to_string(),
            detail: format!(
                "{unmapped_active} active unix sockets on {path} could not be mapped to a process"
            ),
            source: Some(unix_id.to_string()),
            weakens_evidence: true,
        });
    }
    if unmapped_listener > 0 {
        unknowns.push(ImpactUnknown {
            kind: "unmapped_listener_sockets".to_string(),
            detail: format!(
                "{unmapped_listener} unix listener sockets on {path} could not be mapped to a process"
            ),
            source: Some(unix_id.to_string()),
            weakens_evidence: true,
        });
    }
    Ok(unknowns)
}

fn parse_port_node(port_id: &NodeId) -> Result<(String, u16), AppError> {
    let s = port_id.as_str();
    let rest = s
        .strip_prefix("port:tcp:")
        .ok_or_else(|| ImpactError::UnsupportedTarget {
            kind: s.to_string(),
        })?;
    let (ip, port_str) = rest
        .rsplit_once(':')
        .ok_or_else(|| ImpactError::UnsupportedTarget {
            kind: s.to_string(),
        })?;
    let port: u16 = port_str
        .parse()
        .map_err(|_| ImpactError::UnsupportedTarget {
            kind: s.to_string(),
        })?;
    Ok((ip.to_string(), port))
}

fn observation_matches_port(obs: &Observation, ip: &str, port: u16) -> bool {
    let local_ip = obs.metadata().get("local_ip").and_then(|v| v.as_str());
    let local_port = obs.metadata().get("local_port").and_then(|v| v.as_u64());
    let remote_ip = obs.metadata().get("remote_ip").and_then(|v| v.as_str());
    let remote_port = obs.metadata().get("remote_port").and_then(|v| v.as_u64());

    if local_ip == Some(ip) && local_port == Some(port as u64) {
        return true;
    }
    if remote_ip == Some(ip) && remote_port == Some(port as u64) {
        return true;
    }
    if local_ip == Some("0.0.0.0") && local_port == Some(port as u64) {
        return true;
    }
    if local_ip == Some("::") && local_port == Some(port as u64) {
        return true;
    }
    false
}

fn missing_evidence_unknown(edge_id: &str) -> ImpactUnknown {
    ImpactUnknown {
        kind: "missing_evidence".to_string(),
        detail: format!("edge {edge_id} has no readable observation links"),
        source: Some("graph".to_string()),
        weakens_evidence: true,
    }
}

fn score_risk(
    target_kind: NodeKind,
    direct_dependents: &[ImpactDependent],
    configured_dependents: &[ImpactDependent],
    unknowns: &[ImpactUnknown],
) -> RiskAssessment {
    let significant_unknowns = unknowns.iter().any(|u| u.weakens_evidence);
    let count = direct_dependents.len();
    let mut reasons = Vec::new();

    let level = match count {
        0 if !significant_unknowns => {
            reasons.push("no runtime dependents are known".to_string());
            if !configured_dependents.is_empty() {
                reasons.push(format!(
                    "{} configured dependent(s) are inactive or not runtime-active",
                    configured_dependents.len()
                ));
            }
            RiskLevel::Low
        }
        0 => {
            reasons.push("no runtime dependents are known".to_string());
            reasons.push("coverage gaps may hide additional dependents".to_string());
            RiskLevel::Unknown
        }
        1 => {
            reasons.push("1 direct dependent is known".to_string());
            RiskLevel::Medium
        }
        2..=4 => {
            reasons.push(format!("{count} direct dependents are known"));
            RiskLevel::High
        }
        _ => {
            reasons.push(format!("{count} direct dependents are known"));
            RiskLevel::Critical
        }
    };

    if count > 0 && significant_unknowns {
        reasons.push("coverage gaps may hide additional dependents".to_string());
    }
    if target_kind == NodeKind::Service && count > 0 {
        reasons.push("target is a service dependency target".to_string());
    }

    RiskAssessment {
        level: level.to_string(),
        reasons,
    }
}

fn score_evidence(
    direct_dependents: &[ImpactDependent],
    evidence: &[ImpactEvidenceLine],
    unknowns: &[ImpactUnknown],
) -> EvidenceStrength {
    let mut best = 0u8;
    let mut has_observation_links = false;
    let mut only_inferred = true;

    for dependent in direct_dependents {
        if !dependent.observation_ids.is_empty() {
            has_observation_links = true;
        }
        if dependent.edge_class != EdgeClass::Inferred.to_string() {
            only_inferred = false;
        }
        for line in &dependent.evidence {
            let score = EvidenceStrength::new(line.strength_score()).score();
            best = best.max(score);
        }
    }

    for line in evidence {
        let score = EvidenceStrength::new(line.strength_score()).score();
        best = best.max(score);
    }

    if !has_observation_links && direct_dependents.is_empty() && evidence.is_empty() {
        return EvidenceStrength::weak();
    }
    if !has_observation_links {
        best = best.min(30);
    } else if best == 0 {
        best = 30;
    }

    if only_inferred && !direct_dependents.is_empty() {
        best = best.min(85);
    }

    if unknowns.iter().any(|u| u.weakens_evidence) {
        best = best.min(60);
    }

    EvidenceStrength::new(best)
}
