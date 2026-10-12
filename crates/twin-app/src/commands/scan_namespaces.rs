use std::collections::{HashMap, HashSet};
use twin_collectors::{NetworkNamespaceBatch, ProcessBatch};
use twin_core::{
    GraphEdge, GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeState, ObservationId,
    TimestampNs, TransportProtocol,
};
use twin_observation::{Observation, ObservationKind};
use twin_store::{Store, StoreError};

use super::{
    scan::ScanEdgeCache,
    scan_history::{self, ScanHistorySession},
};

pub(crate) struct NamespaceScanCounts {
    pub(crate) endpoints: HashSet<NodeId>,
    pub(crate) edges: HashSet<(NodeId, twin_core::EdgeKind, NodeId, bool)>,
}

pub(crate) fn persist(
    store: &mut Store,
    history: &mut ScanHistorySession,
    cache: &mut ScanEdgeCache,
    batch: &ProcessBatch,
    observations: &[Observation],
    at: TimestampNs,
) -> Result<NamespaceScanCounts, StoreError> {
    let mut counts = NamespaceScanCounts {
        endpoints: HashSet::new(),
        edges: HashSet::new(),
    };
    let services = super::scan::services_by_pid(batch.records());
    let boot_id = batch.records().iter().find_map(|record| record.boot_id());
    for namespace in batch.namespaces() {
        let mut listeners: HashMap<NodeId, Vec<(NodeId, ObservationId)>> = HashMap::new();
        let selected = observations
            .iter()
            .filter(|observation| {
                observation
                    .metadata()
                    .get("network_namespace")
                    .and_then(|value| value.as_str())
                    == Some(namespace.namespace.to_string().as_str())
            })
            .collect::<Vec<_>>();
        for observation in selected
            .iter()
            .copied()
            .filter(|obs| is_listener(obs.kind()))
        {
            let Some(port) = observation.subject() else {
                continue;
            };
            endpoint(store, history, port, observation, at, boot_id)?;
            counts.endpoints.insert(port.clone());
            for process in owners(namespace, observation) {
                let edge = GraphEdge::observed_process_listens_on(&process, port, at, None);
                save(
                    store,
                    history,
                    cache,
                    edge,
                    observation,
                    (namespace.namespace, boot_id),
                    &mut counts,
                )?;
                for service in services
                    .get(&process.process_pid().unwrap_or(0))
                    .into_iter()
                    .flatten()
                {
                    let edge = GraphEdge::inferred_service_listens_on(service, port, at, None);
                    save(
                        store,
                        history,
                        cache,
                        edge,
                        observation,
                        (namespace.namespace, boot_id),
                        &mut counts,
                    )?;
                    listeners
                        .entry(port.clone())
                        .or_default()
                        .push((service.clone(), observation.id()));
                }
            }
        }
        for observation in selected
            .iter()
            .copied()
            .filter(|obs| is_connection(obs.kind()))
        {
            let Some(port) = observation.subject() else {
                continue;
            };
            endpoint(store, history, port, observation, at, boot_id)?;
            counts.endpoints.insert(port.clone());
            for process in owners(namespace, observation) {
                let edge = GraphEdge::observed_process_connects_to(&process, port, at, None);
                save(
                    store,
                    history,
                    cache,
                    edge,
                    observation,
                    (namespace.namespace, boot_id),
                    &mut counts,
                )?;
                for service in services
                    .get(&process.process_pid().unwrap_or(0))
                    .into_iter()
                    .flatten()
                {
                    let edge = GraphEdge::inferred_service_connects_to(service, port, at, None);
                    save(
                        store,
                        history,
                        cache,
                        edge,
                        observation,
                        (namespace.namespace, boot_id),
                        &mut counts,
                    )?;
                    for candidate in candidates(namespace, port, observation) {
                        for (target, listener_observation) in
                            listeners.get(&candidate).into_iter().flatten()
                        {
                            if service == target {
                                continue;
                            }
                            let edge = if port.kind() == Some(twin_core::NodeKind::UnixSocket) {
                                GraphEdge::inferred_service_depends_on_unix(
                                    service, target, at, None,
                                )
                            } else {
                                GraphEdge::inferred_service_depends_on(service, target, at, None)
                            };
                            let id = edge.id().to_string();
                            save(
                                store,
                                history,
                                cache,
                                edge,
                                observation,
                                (namespace.namespace, boot_id),
                                &mut counts,
                            )?;
                            store.link_edge_observation(
                                &id,
                                &listener_observation.to_string(),
                                "support",
                            )?;
                        }
                    }
                }
            }
        }
    }
    Ok(counts)
}

fn owners(namespace: &NetworkNamespaceBatch, observation: &Observation) -> Vec<NodeId> {
    let inode = observation
        .metadata()
        .get("inode")
        .and_then(|value| value.as_str())
        .and_then(|value| value.parse::<u64>().ok());
    inode
        .and_then(|inode| namespace.owners.get(&inode))
        .into_iter()
        .flatten()
        .map(|owner| NodeId::process(owner.pid))
        .collect()
}

fn endpoint(
    store: &mut Store,
    history: &mut ScanHistorySession,
    id: &NodeId,
    observation: &Observation,
    at: TimestampNs,
    boot_id: Option<&str>,
) -> Result<(), StoreError> {
    let existing = store.get_node_typed(id)?;
    let previous: serde_json::Value = existing
        .as_ref()
        .and_then(|node| serde_json::from_str(node.metadata().as_str()).ok())
        .unwrap_or_default();
    let changed_boot = previous
        .get("boot_id")
        .and_then(|value| value.as_str())
        .zip(boot_id)
        .is_some_and(|(old, new)| old != new);
    let changed = changed_boot
        || existing
            .as_ref()
            .is_some_and(|node| node.state() == NodeState::Gone);
    if changed {
        let mut edges = store.list_active_edges_from(id.as_str())?;
        edges.extend(store.list_active_edges_to(id.as_str())?);
        let ids: Vec<_> = edges.into_iter().map(|row| row.id).collect();
        store.mark_edges_missing(&ids, at.as_i64(), false, history.collector_run_id())?;
        store.mark_nodes_missing(
            &[id.to_string()],
            at.as_i64(),
            false,
            history.collector_run_id(),
        )?;
        store.reset_node_incarnation(id, at)?;
    }
    let retained_boot_id =
        boot_id.or_else(|| previous.get("boot_id").and_then(|value| value.as_str()));
    let existing = existing.as_ref().filter(|_| !changed);
    let protocol = match observation.kind() {
        ObservationKind::UdpSocketSeen | ObservationKind::UdpConnectionSeen => Some("udp"),
        ObservationKind::TcpSocketSeen | ObservationKind::TcpConnectionSeen => Some("tcp"),
        _ => None,
    };
    let node = GraphNode::from_parts(GraphNodeParts {
        id: id.clone(), kind: id.kind().ok_or_else(|| StoreError::Decode { detail: "namespace endpoint has no kind".into() })?, label: id.to_string(), state: NodeState::Active,
        first_seen: existing.map(|node| node.first_seen()).unwrap_or(at), last_seen: at,
        valid_from: existing.map(|node| node.valid_from()).unwrap_or(at), valid_to: None,
        metadata: GraphMetadata::from_json(serde_json::json!({"source": observation.source().to_string(), "network_namespace": id.network_namespace().map(|namespace| namespace.to_string()), "protocol": protocol, "boot_id": retained_boot_id, "boot_identity_known": boot_id.is_some() }).to_string()),
    });
    scan_history::upsert_node(store, history, &node)
}

fn save(
    store: &mut Store,
    history: &mut ScanHistorySession,
    cache: &mut ScanEdgeCache,
    edge: GraphEdge,
    observation: &Observation,
    identity: (twin_core::NetworkNamespaceId, Option<&str>),
    counts: &mut NamespaceScanCounts,
) -> Result<(), StoreError> {
    counts.edges.insert((
        edge.from().clone(),
        edge.kind(),
        edge.to().clone(),
        edge.to().kind() == Some(twin_core::NodeKind::UnixSocket)
            || edge.metadata().as_str().contains("unix_connection"),
    ));
    let mut existing =
        super::scan::load_existing_edge(store, cache, edge.from(), edge.kind(), edge.to())?;
    let previous: serde_json::Value = existing
        .as_ref()
        .and_then(|edge| serde_json::from_str(edge.metadata().as_str()).ok())
        .unwrap_or_default();
    let changed_boot = previous
        .get("boot_id")
        .and_then(|value| value.as_str())
        .zip(identity.1)
        .is_some_and(|(old, new)| old != new);
    if changed_boot {
        store.mark_edges_missing(
            &[edge.id().to_string()],
            edge.last_seen().as_i64(),
            false,
            history.collector_run_id(),
        )?;
        existing = None;
    }
    if changed_boot
        || existing
            .as_ref()
            .is_some_and(|edge| edge.state() == twin_core::EdgeState::Gone)
    {
        store.reset_edge_incarnation(edge.id(), edge.last_seen())?;
    }
    let existing = existing.filter(|edge| edge.state() != twin_core::EdgeState::Gone);
    let mut metadata = serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(
        edge.metadata().as_str(),
    )
    .unwrap_or_default();
    metadata.insert(
        "source".into(),
        serde_json::Value::String(observation.source().to_string()),
    );
    metadata.insert(
        "network_namespace".into(),
        serde_json::Value::String(identity.0.to_string()),
    );
    metadata.insert(
        "boot_id".into(),
        identity
            .1
            .or_else(|| previous.get("boot_id").and_then(|value| value.as_str()))
            .map(|value| serde_json::Value::String(value.into()))
            .unwrap_or(serde_json::Value::Null),
    );
    metadata.insert(
        "boot_identity_known".into(),
        serde_json::Value::Bool(identity.1.is_some()),
    );
    let edge = GraphEdge::from_stored(twin_core::GraphEdgeParts {
        from: edge.from().clone(),
        to: edge.to().clone(),
        kind: edge.kind(),
        class: edge.class(),
        state: edge.state(),
        evidence_count: existing
            .as_ref()
            .map(|edge| edge.evidence_count().saturating_add(1))
            .unwrap_or(1),
        first_seen: existing
            .as_ref()
            .map(|edge| edge.first_seen())
            .unwrap_or(edge.first_seen()),
        last_seen: edge.last_seen(),
        metadata: GraphMetadata::from_json(serde_json::Value::Object(metadata).to_string()),
    });
    super::scan::upsert_edge_with_link(store, history, &edge, Some((&observation.id(), "direct")))
}

fn is_listener(kind: ObservationKind) -> bool {
    matches!(
        kind,
        ObservationKind::TcpSocketSeen
            | ObservationKind::UdpSocketSeen
            | ObservationKind::UnixSocketSeen
    )
}
fn is_connection(kind: ObservationKind) -> bool {
    matches!(
        kind,
        ObservationKind::TcpConnectionSeen
            | ObservationKind::UdpConnectionSeen
            | ObservationKind::UnixConnectionSeen
    )
}

fn candidates(
    namespace: &NetworkNamespaceBatch,
    port: &NodeId,
    observation: &Observation,
) -> Vec<NodeId> {
    if port.kind() == Some(twin_core::NodeKind::UnixSocket) {
        return vec![port.clone()];
    }
    let Some(ip) = observation
        .metadata()
        .get("remote_ip")
        .and_then(|value| value.as_str())
    else {
        return Vec::new();
    };
    let Some(port) = observation
        .metadata()
        .get("remote_port")
        .and_then(|value| value.as_u64())
        .and_then(|port| u16::try_from(port).ok())
    else {
        return Vec::new();
    };
    let protocol = if observation.kind() == ObservationKind::UdpConnectionSeen {
        TransportProtocol::Udp
    } else {
        TransportProtocol::Tcp
    };
    let wildcard = if ip.contains(':') { "::" } else { "0.0.0.0" };
    let is_local = ip
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback())
        || namespace
            .inet_listeners
            .iter()
            .any(|socket| socket.local_ip == ip)
        || namespace
            .inet_connections
            .iter()
            .any(|socket| socket.local_ip == ip);
    std::iter::once(ip)
        .chain(is_local.then_some(wildcard))
        .filter_map(|ip| {
            NodeId::inet_port_in_namespace(namespace.namespace, protocol, ip, port).ok()
        })
        .collect()
}
