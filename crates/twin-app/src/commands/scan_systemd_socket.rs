use std::collections::HashSet;

use twin_core::{
    GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState, TimestampNs,
};
use twin_observation::{Observation, ObservationKind};
use twin_store::{Store, StoreError};

pub(crate) fn persist_socket_activation_in_scan(
    store: &mut Store,
    edge_cache: &mut super::scan::ScanEdgeCache,
    observations: &[Observation],
    scan_time: TimestampNs,
    seen_service_depends: &mut HashSet<(NodeId, NodeId)>,
) -> Result<usize, StoreError> {
    let mut count = 0usize;
    for obs in observations {
        if obs.kind() != ObservationKind::SystemdSocketActivates {
            continue;
        }
        let Some(socket_unit) = obs.metadata().get("socket_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(service_unit) = obs.metadata().get("service_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        let unit_path = obs
            .metadata()
            .get("unit_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let socket_id = NodeId::service(socket_unit);
        let service_id = NodeId::service(service_unit);
        for unit in [socket_unit, service_unit] {
            upsert_service_unit_node(store, unit, scan_time)?;
        }
        super::scan_systemd_groups::persist_depends_on_group(
            store,
            edge_cache,
            &socket_id,
            &service_id,
            super::scan_systemd_groups::EdgeObservationLink::Single(&obs.id(), "direct"),
            |existing| {
                twin_core::GraphEdge::observed_socket_activates_service(
                    &socket_id,
                    &service_id,
                    scan_time,
                    existing,
                    unit_path,
                )
            },
        )?;
        if seen_service_depends.insert((socket_id.clone(), service_id.clone())) {
            count += 1;
        }
    }
    Ok(count)
}

fn upsert_service_unit_node(
    store: &mut Store,
    unit: &str,
    scan_time: TimestampNs,
) -> Result<(), StoreError> {
    let id = NodeId::service(unit);
    let existing = store.get_node_typed(&id)?;
    let (first_seen, valid_from) = match existing.as_ref() {
        Some(node) => (node.first_seen(), node.valid_from()),
        None => (scan_time, scan_time),
    };
    let label = id
        .as_str()
        .strip_prefix("service:")
        .unwrap_or(unit)
        .to_string();
    let node = GraphNode::from_parts(GraphNodeParts {
        id,
        kind: NodeKind::Service,
        label,
        state: NodeState::Active,
        first_seen,
        last_seen: scan_time,
        valid_from,
        valid_to: None,
        metadata: service_unit_metadata(unit, existing.as_ref()),
    });
    store.upsert_node_typed(&node)
}

fn service_unit_metadata(unit: &str, existing: Option<&GraphNode>) -> GraphMetadata {
    let unit_type = if unit.ends_with(".socket") {
        "socket"
    } else {
        "service"
    };
    let base = existing
        .map(|n| n.metadata().as_str())
        .unwrap_or(r#"{"source":"systemd_cgroup_inference"}"#);
    let mut value: serde_json::Value = match serde_json::from_str(base) {
        Ok(v) => v,
        Err(_) => serde_json::json!({}),
    };
    if let Some(obj) = value.as_object_mut() {
        obj.insert(
            "unit_type".to_string(),
            serde_json::Value::String(unit_type.to_string()),
        );
    }
    GraphMetadata::from_json(value.to_string())
}
