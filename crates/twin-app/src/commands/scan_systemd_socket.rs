use std::collections::HashSet;

use twin_core::{
    GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState, TimestampNs,
};
use twin_observation::{Observation, ObservationKind};
use twin_store::{Store, StoreError};

/// What starts a service: a socket on connection or a timer on schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Activation {
    Socket,
    Timer,
}

impl Activation {
    fn observation_kind(self) -> ObservationKind {
        match self {
            Self::Socket => ObservationKind::SystemdSocketActivates,
            Self::Timer => ObservationKind::SystemdTimerActivates,
        }
    }

    fn activator_key(self) -> &'static str {
        match self {
            Self::Socket => "socket_unit",
            Self::Timer => "timer_unit",
        }
    }
}

pub(crate) fn persist_activation_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut super::scan::ScanEdgeCache,
    observations: &[Observation],
    scan_time: TimestampNs,
    seen_service_depends: &mut HashSet<(NodeId, NodeId)>,
    activation: Activation,
) -> Result<usize, StoreError> {
    let mut count = 0usize;
    for obs in observations {
        if obs.kind() != activation.observation_kind() {
            continue;
        }
        let Some(socket_unit) = obs
            .metadata()
            .get(activation.activator_key())
            .and_then(|v| v.as_str())
        else {
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
            upsert_service_unit_node(store, history, unit, scan_time)?;
        }
        super::scan_systemd_groups::persist_depends_on_group(
            store,
            history,
            edge_cache,
            &socket_id,
            &service_id,
            super::scan_systemd_groups::EdgeObservationLink::Single(&obs.id(), "direct"),
            |existing| {
                twin_core::GraphEdge::observed_activates_service(
                    &socket_id,
                    &service_id,
                    scan_time,
                    existing,
                    unit_path,
                ).with_metadata(twin_core::GraphMetadata::from_json(match activation {
                    Activation::Socket => serde_json::json!({
                        "source": obs.source().to_string(), "socket_activation": true,
                        "unit_path": unit_path, "accept": obs.metadata().get("accept").and_then(|value| value.as_bool()),
                    }),
                    Activation::Timer => serde_json::json!({
                        "source": obs.source().to_string(), "timer_activation": true,
                        "unit_path": unit_path,
                    }),
                }.to_string()))
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
    history: &mut super::scan_history::ScanHistorySession,
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
    super::scan_history::upsert_node(store, history, &node)
}

fn service_unit_metadata(unit: &str, existing: Option<&GraphNode>) -> GraphMetadata {
    let unit_type = if unit.ends_with(".socket") {
        "socket"
    } else if unit.ends_with(".timer") {
        "timer"
    } else if unit.ends_with("@.service") {
        "service_template"
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
