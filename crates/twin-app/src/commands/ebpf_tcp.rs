use std::str::FromStr;

use twin_core::{
    score_evidence, EdgeId, EdgeKind, EvidenceFactors, EvidenceRecency, EvidenceSourceKind,
    EvidenceStrength, GraphEdge, GraphNode, NodeId, NodeKind, ObservationId, TimestampNs,
};
use twin_ebpf::{
    dropped_observation, tcp_to_raw, TcpConnect, TcpEvent, TcpRateLimiter, TcpRateLimits,
};
use twin_observation::{Observation, ObservationKind, Pipeline};
use twin_store::Store;

use crate::error::{AppError, ScanError};
use crate::model::{ImpactEvidenceLine, RuntimeDependency, WatchTcpEvent};

use super::scan::listener_port_candidates;

pub struct TcpIngestor {
    limiter: TcpRateLimiter,
}

impl TcpIngestor {
    pub fn new(limits: TcpRateLimits) -> Self {
        Self {
            limiter: TcpRateLimiter::new(limits),
        }
    }

    pub fn dropped(&self) -> u64 {
        self.limiter.dropped()
    }

    pub fn push(
        &mut self,
        store: &mut Store,
        event: &TcpEvent,
    ) -> Result<Option<WatchTcpEvent>, AppError> {
        if !self.limiter.admit(event.pid(), &event.endpoint()) {
            return Ok(None);
        }
        let observation = Pipeline::default()
            .process(tcp_to_raw(event))
            .map_err(ScanError::Observation)?;
        ensure_process(store, event.pid(), observation.timestamp())?;
        if let Some(port) = observation.object() {
            ensure_port(store, port, observation.timestamp())?;
        }
        store
            .insert_observation_typed(&observation)
            .map_err(ScanError::Store)?;
        if let TcpEvent::Connect(connect) = event {
            apply_connect(store, &observation, connect)?;
        }
        Ok(Some(WatchTcpEvent {
            node_id: format!("process:pid:{}", event.pid()),
            action: event.action().to_string(),
            endpoint: event.endpoint(),
        }))
    }

    pub fn sync_drops(&self, store: &mut Store, timestamp_ns: i64) -> Result<(), AppError> {
        let count = self.limiter.dropped();
        if count == 0 {
            return Ok(());
        }
        let rows = store
            .list_observations_by_kind(&ObservationKind::EbpfDroppedEvents.to_string())
            .map_err(ScanError::Store)?;
        for row in &rows {
            store
                .delete_observation(&row.id)
                .map_err(ScanError::Store)?;
        }
        let observation = Pipeline::default()
            .process(dropped_observation(count, timestamp_ns))
            .map_err(ScanError::Observation)?;
        store
            .insert_observation_typed(&observation)
            .map_err(ScanError::Store)?;
        Ok(())
    }
}

pub fn connect_evidence_strength(
    ebpf_connects: u64,
    socket_table: bool,
    config_only: bool,
    dropped: bool,
) -> EvidenceStrength {
    let source = if ebpf_connects > 0 {
        EvidenceSourceKind::Ebpf
    } else if socket_table {
        EvidenceSourceKind::SocketTable
    } else if config_only {
        EvidenceSourceKind::Config
    } else {
        EvidenceSourceKind::None
    };
    let mut independent_sources = 0u32;
    if ebpf_connects > 0 {
        independent_sources += 1;
    }
    if socket_table {
        independent_sources += 1;
    }
    if config_only {
        independent_sources += 1;
    }
    let repeat_count = if ebpf_connects > 0 {
        u32::try_from(ebpf_connects).unwrap_or(u32::MAX)
    } else if socket_table || config_only {
        1
    } else {
        0
    };
    score_evidence(EvidenceFactors {
        source,
        runtime_confirmed: socket_table && ebpf_connects > 0,
        static_confirmed: false,
        recency: EvidenceRecency::Unknown,
        repeat_count,
        independent_sources,
        permission_gaps: 0,
        conflicting: false,
        dropped_ebpf: dropped,
    })
    .strength
}

pub(crate) fn runtime_dependencies(
    store: &Store,
    service: &NodeId,
) -> Result<Vec<RuntimeDependency>, AppError> {
    let dropped = tcp_drop_count(store).map_err(ScanError::Store)? > 0;
    let rows = store
        .list_active_edges_from(service.as_str())
        .map_err(ScanError::Store)?;
    let mut out = Vec::new();
    for row in rows {
        let edge = GraphEdge::try_from(&row).map_err(ScanError::Store)?;
        if edge.kind() != EdgeKind::ConnectsTo {
            continue;
        }
        let (connects, socket) = connect_counts(store, edge.id().as_str())?;
        if connects == 0 {
            continue;
        }
        let listener = edge.to().kind() == Some(NodeKind::Service);
        let strength = connect_evidence_strength(connects, socket, false, dropped);
        let mut reasons = vec![connect_reason(connects)];
        if listener {
            reasons.push("socket inode mapping confirmed listener ownership".to_string());
        }
        out.push(RuntimeDependency {
            from_id: edge.from().to_string(),
            to_id: edge.to().to_string(),
            relationship: edge.kind().to_string(),
            evidence_label: strength.label().to_string(),
            evidence_score: strength.score(),
            reasons,
        });
    }
    out.sort_by(|left, right| left.to_id.cmp(&right.to_id));
    Ok(out)
}

pub(crate) fn ebpf_connect_evidence(
    store: &Store,
    observation_ids: &[String],
    relationship: &str,
) -> Option<ImpactEvidenceLine> {
    let mut connects = 0u64;
    let mut socket = false;
    let mut sample = None;
    for obs_id in observation_ids {
        let Ok(id) = ObservationId::from_str(obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        match obs.kind() {
            ObservationKind::EbpfConnect => {
                connects += 1;
                sample = Some(obs.id().to_string());
            }
            ObservationKind::TcpConnectionSeen
            | ObservationKind::TcpSocketSeen
            | ObservationKind::UnixConnectionSeen => {
                socket = true;
            }
            _ => {}
        }
    }
    if connects == 0 {
        return None;
    }
    let dropped = tcp_drop_count(store).unwrap_or(0) > 0;
    let strength = connect_evidence_strength(connects, socket, false, dropped);
    Some(ImpactEvidenceLine {
        source: "ebpf".to_string(),
        statement: connect_reason(connects),
        relationship: relationship.to_string(),
        strength: strength.label().to_string(),
        observation_id: sample,
    })
}

fn connect_counts(store: &Store, edge_id: &str) -> Result<(u64, bool), AppError> {
    let links = store
        .list_observations_for_edge(edge_id)
        .map_err(ScanError::Store)?;
    let mut connects = 0u64;
    let mut socket = false;
    for (obs_id, _) in links {
        let Ok(id) = ObservationId::from_str(&obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        match obs.kind() {
            ObservationKind::EbpfConnect => connects += 1,
            ObservationKind::TcpConnectionSeen | ObservationKind::TcpSocketSeen => socket = true,
            _ => {}
        }
    }
    Ok((connects, socket))
}

fn apply_connect(
    store: &mut Store,
    observation: &Observation,
    event: &TcpConnect,
) -> Result<(), AppError> {
    let process = NodeId::process(event.pid());
    let port = NodeId::port_tcp(&event.daddr().to_string(), event.dport()).map_err(|err| {
        ScanError::Store(twin_store::StoreError::Decode {
            detail: err.to_string(),
        })
    })?;
    let seen = observation.timestamp();
    upsert_process_connect(store, &process, &port, seen, observation.id())?;
    let owners = owning_services(store, &process)?;
    if owners.is_empty() {
        return Ok(());
    }
    let listeners = listener_services(store, &event.daddr().to_string(), event.dport())?;
    for owner in owners {
        if listeners.is_empty() {
            upsert_service_connect(store, &owner, &port, seen, observation.id())?;
            continue;
        }
        for listener in &listeners {
            upsert_service_connect(store, &owner, listener, seen, observation.id())?;
        }
    }
    Ok(())
}

fn upsert_process_connect(
    store: &mut Store,
    process: &NodeId,
    port: &NodeId,
    seen: TimestampNs,
    observation: ObservationId,
) -> Result<(), AppError> {
    let existing = load_edge(store, process, EdgeKind::ConnectsTo, port)?;
    let edge = GraphEdge::observed_process_connects_to(process, port, seen, existing.as_ref());
    store.upsert_edge_typed(&edge).map_err(ScanError::Store)?;
    store
        .link_edge_observation(edge.id().as_str(), &observation.to_string(), "direct")
        .map_err(ScanError::Store)?;
    Ok(())
}

fn upsert_service_connect(
    store: &mut Store,
    service: &NodeId,
    target: &NodeId,
    seen: TimestampNs,
    observation: ObservationId,
) -> Result<(), AppError> {
    let existing = load_edge(store, service, EdgeKind::ConnectsTo, target)?;
    let edge = GraphEdge::inferred_service_connects_to(service, target, seen, existing.as_ref());
    store.upsert_edge_typed(&edge).map_err(ScanError::Store)?;
    store
        .link_edge_observation(edge.id().as_str(), &observation.to_string(), "support")
        .map_err(ScanError::Store)?;
    Ok(())
}

fn load_edge(
    store: &Store,
    from: &NodeId,
    kind: EdgeKind,
    to: &NodeId,
) -> Result<Option<GraphEdge>, AppError> {
    let id = EdgeId::new(from, kind, to);
    match store.get_edge(id.as_str()).map_err(ScanError::Store)? {
        Some(row) => Ok(Some(GraphEdge::try_from(&row).map_err(ScanError::Store)?)),
        None => Ok(None),
    }
}

fn owning_services(store: &Store, process: &NodeId) -> Result<Vec<NodeId>, AppError> {
    let rows = store
        .list_active_edges_to(process.as_str())
        .map_err(ScanError::Store)?;
    let mut owners = Vec::new();
    for row in rows {
        let edge = GraphEdge::try_from(&row).map_err(ScanError::Store)?;
        if edge.kind() != EdgeKind::Owns {
            continue;
        }
        if edge.from().kind() != Some(NodeKind::Service) {
            continue;
        }
        if !owners.iter().any(|id: &NodeId| id == edge.from()) {
            owners.push(edge.from().clone());
        }
    }
    Ok(owners)
}

fn listener_services(
    store: &Store,
    remote_ip: &str,
    remote_port: u16,
) -> Result<Vec<NodeId>, AppError> {
    let mut listeners = Vec::new();
    for port in listener_port_candidates(remote_ip, remote_port) {
        let rows = store
            .list_active_edges_to(port.as_str())
            .map_err(ScanError::Store)?;
        for row in rows {
            let edge = GraphEdge::try_from(&row).map_err(ScanError::Store)?;
            if edge.kind() != EdgeKind::ListensOn {
                continue;
            }
            if edge.from().kind() != Some(NodeKind::Service) {
                continue;
            }
            if !listeners.iter().any(|id: &NodeId| id == edge.from()) {
                listeners.push(edge.from().clone());
            }
        }
    }
    Ok(listeners)
}

fn ensure_process(store: &mut Store, pid: u32, seen: TimestampNs) -> Result<(), AppError> {
    let id = NodeId::process(pid);
    if store
        .get_node_typed(&id)
        .map_err(ScanError::Store)?
        .is_some()
    {
        return Ok(());
    }
    let node = GraphNode::process(pid, format!("pid:{pid}"), seen, None);
    store.upsert_node_typed(&node).map_err(ScanError::Store)?;
    Ok(())
}

fn ensure_port(store: &mut Store, port: &NodeId, seen: TimestampNs) -> Result<(), AppError> {
    if port.kind() != Some(NodeKind::Port) {
        return Ok(());
    }
    if store
        .get_node_typed(port)
        .map_err(ScanError::Store)?
        .is_some()
    {
        return Ok(());
    }
    let Some((ip, number)) = port_parts(port) else {
        return Ok(());
    };
    let node = GraphNode::tcp_port(&ip, number, seen, None).map_err(|err| {
        ScanError::Store(twin_store::StoreError::Decode {
            detail: err.to_string(),
        })
    })?;
    store.upsert_node_typed(&node).map_err(ScanError::Store)?;
    Ok(())
}

fn port_parts(port: &NodeId) -> Option<(String, u16)> {
    let rest = port.as_str().strip_prefix("port:tcp:")?;
    if let Some(rest) = rest.strip_prefix('[') {
        let (ip, number) = rest.rsplit_once("]:")?;
        let number = number.parse().ok()?;
        return Some((ip.to_string(), number));
    }
    let (ip, number) = rest.rsplit_once(':')?;
    let number = number.parse().ok()?;
    Some((ip.to_string(), number))
}

pub(crate) fn ebpf_drops_recorded(store: &Store) -> Result<bool, twin_store::StoreError> {
    Ok(tcp_drop_count(store)? > 0)
}

fn tcp_drop_count(store: &Store) -> Result<u64, twin_store::StoreError> {
    let rows = store.list_observations_by_kind(&ObservationKind::EbpfDroppedEvents.to_string())?;
    let mut total = 0u64;
    for row in rows {
        if row.source != "ebpf" {
            continue;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&row.metadata_json) else {
            continue;
        };
        if value.get("source").and_then(|item| item.as_str()) != Some("ebpf_tcp") {
            continue;
        }
        let count = value
            .get("count")
            .and_then(|item| item.as_u64())
            .unwrap_or(0);
        total = total.saturating_add(count);
    }
    Ok(total)
}

fn connect_reason(count: u64) -> String {
    if count == 1 {
        "eBPF observed 1 connect event".to_string()
    } else {
        format!("eBPF observed {count} connect events")
    }
}
