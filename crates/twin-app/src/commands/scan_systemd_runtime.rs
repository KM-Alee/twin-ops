use std::collections::{HashMap, HashSet};

use twin_collectors::{SystemdRuntimeBatch, RUNTIME_COLLECTOR_NAME};
use twin_core::{
    EdgeKind, GraphEdge, GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState,
    ObservationId, TimestampNs,
};
use twin_observation::{Observation, ObservationKind, ObservationSource};
use twin_store::{CollectorRunRow, Store, StoreError};

pub(crate) fn persist_runtime_in_scan(
    store: &mut Store,
    batch: &SystemdRuntimeBatch,
    observations: &[Observation],
    scan_time: TimestampNs,
    enable_depends_on_edge_count: &mut usize,
    dbus_depends_on_edge_count: &mut usize,
    seen_service_depends: &mut HashSet<(NodeId, NodeId)>,
) -> Result<(), StoreError> {
    let dep_groups = runtime_dependency_groups(observations);
    let obs_by_id: HashMap<ObservationId, &Observation> =
        observations.iter().map(|o| (o.id(), o)).collect();

    let run_id = store.insert_collector_run(&CollectorRunRow {
        id: None,
        collector: RUNTIME_COLLECTOR_NAME.to_string(),
        started_at_ns: batch.started_at().as_i64(),
        ended_at_ns: batch.ended_at().as_i64(),
        status: "success".to_string(),
        observation_count: observations.len() as i64,
        warning_count: batch.warnings().len() as i64,
        error_message: None,
        metadata_json: serde_json::json!({
            "dbus_available": batch.dbus_available(),
            "dbus_unit_count": batch.dbus_unit_count(),
            "enable_symlink_count": batch.enable_symlink_count(),
        })
        .to_string(),
    })?;

    for obs in observations {
        store.insert_observation_typed_for_run(obs, run_id)?;
    }

    for obs in observations {
        if obs.kind() != ObservationKind::SystemdUnitStateSeen {
            continue;
        }
        let Some(unit) = obs.metadata().get("unit").and_then(|v| v.as_str()) else {
            continue;
        };
        upsert_service_state_node(store, unit, obs, scan_time)?;
    }

    for ((from_unit, to_unit), obs_ids) in &dep_groups {
        let source_id = NodeId::service(from_unit);
        let target_id = NodeId::service(to_unit);
        for unit in [from_unit.as_str(), to_unit.as_str()] {
            let id = NodeId::service(unit);
            let existing = store.get_node_typed(&id)?;
            if let Some(state_obs) = obs_ids.iter().find_map(|oid| {
                obs_by_id
                    .get(oid)
                    .filter(|o| o.kind() == ObservationKind::SystemdUnitStateSeen)
            }) {
                upsert_service_state_node(store, unit, state_obs, scan_time)?;
            } else if existing.is_none() {
                store.upsert_node_typed(&GraphNode::service(unit, scan_time, None))?;
            }
        }
        let Some(primary) = obs_ids.iter().find_map(|id| obs_by_id.get(id)) else {
            continue;
        };
        let dep_key = primary
            .metadata()
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("Requires");
        let existing_edge =
            super::scan::load_existing_edge(store, &source_id, EdgeKind::DependsOn, &target_id)?;

        let edge = match primary.kind() {
            ObservationKind::SystemdUnitWantedBy => {
                let symlink_path = primary
                    .metadata()
                    .get("symlink_path")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let enable_kind = primary
                    .metadata()
                    .get("enable_kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or("wants");
                GraphEdge::observed_service_depends_on_enable(
                    &source_id,
                    &target_id,
                    scan_time,
                    existing_edge.as_ref(),
                    enable_kind,
                    symlink_path,
                )
            }
            _ if primary.source() == ObservationSource::SystemdDBus => {
                GraphEdge::observed_service_depends_on_dbus(
                    &source_id,
                    &target_id,
                    scan_time,
                    existing_edge.as_ref(),
                    dep_key,
                )
            }
            _ => continue,
        };

        super::scan::upsert_edge_with_link(store, &edge, None)?;
        for obs_id in obs_ids {
            store.link_edge_observation(edge.id().as_str(), &obs_id.to_string(), "direct")?;
        }
        if seen_service_depends.insert((source_id.clone(), target_id.clone())) {
            match primary.kind() {
                ObservationKind::SystemdUnitWantedBy => *enable_depends_on_edge_count += 1,
                _ => *dbus_depends_on_edge_count += 1,
            }
        }
    }

    Ok(())
}

fn runtime_dependency_groups(
    observations: &[Observation],
) -> HashMap<(String, String), Vec<ObservationId>> {
    let mut map: HashMap<(String, String), Vec<ObservationId>> = HashMap::new();
    for obs in observations {
        let group = match obs.kind() {
            ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants => {
                if obs.metadata().get("source").and_then(|v| v.as_str()) != Some("systemd_dbus") {
                    continue;
                }
                true
            }
            ObservationKind::SystemdUnitWantedBy => true,
            _ => false,
        };
        if !group {
            continue;
        }
        let Some(from_unit) = obs.metadata().get("from_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(to_unit) = obs.metadata().get("to_unit").and_then(|v| v.as_str()) else {
            continue;
        };
        map.entry((from_unit.to_string(), to_unit.to_string()))
            .or_default()
            .push(obs.id());
    }
    map
}

fn upsert_service_state_node(
    store: &mut Store,
    unit: &str,
    obs: &Observation,
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
    let mut meta = serde_json::Map::new();
    meta.insert(
        "source".to_string(),
        serde_json::Value::String("systemd_dbus".to_string()),
    );
    if let Some(v) = obs.metadata().get("unit_type").and_then(|v| v.as_str()) {
        meta.insert(
            "unit_type".to_string(),
            serde_json::Value::String(v.to_string()),
        );
    } else if unit.ends_with(".socket") {
        meta.insert(
            "unit_type".to_string(),
            serde_json::Value::String("socket".to_string()),
        );
    } else {
        meta.insert(
            "unit_type".to_string(),
            serde_json::Value::String("service".to_string()),
        );
    }
    for key in ["active_state", "load_state", "sub_state"] {
        if let Some(v) = obs.metadata().get(key).and_then(|v| v.as_str()) {
            meta.insert(key.to_string(), serde_json::Value::String(v.to_string()));
        }
    }
    let node = GraphNode::from_parts(GraphNodeParts {
        id,
        kind: NodeKind::Service,
        label,
        state: NodeState::Active,
        first_seen,
        last_seen: scan_time,
        valid_from,
        valid_to: None,
        metadata: GraphMetadata::from_json(serde_json::Value::Object(meta).to_string()),
    });
    store.upsert_node_typed(&node)
}
