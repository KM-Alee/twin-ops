use std::collections::{HashMap, HashSet};

use twin_collectors::{SystemdUnitBatch, SYSTEMD_UNIT_COLLECTOR_NAME};
use twin_core::{EdgeKind, GraphEdge, GraphNode, NodeId, ObservationId, TimestampNs};
use twin_observation::{Observation, ObservationKind};
use twin_store::{CollectorRunRow, Store, StoreError};

pub(crate) fn persist_systemd_in_scan(
    store: &mut Store,
    batch: &SystemdUnitBatch,
    observations: &[Observation],
    scan_time: TimestampNs,
    declared_depends_on_edge_count: &mut usize,
    seen_service_depends: &mut HashSet<(NodeId, NodeId)>,
) -> Result<usize, StoreError> {
    let dep_groups = systemd_dependency_groups(observations);
    let obs_by_id: HashMap<ObservationId, &Observation> =
        observations.iter().map(|o| (o.id(), o)).collect();

    let run_id = store.insert_collector_run(&CollectorRunRow {
        id: None,
        collector: SYSTEMD_UNIT_COLLECTOR_NAME.to_string(),
        started_at_ns: batch.started_at().as_i64(),
        ended_at_ns: batch.ended_at().as_i64(),
        status: "success".to_string(),
        observation_count: observations.len() as i64,
        warning_count: batch.warnings().len() as i64,
        error_message: None,
        metadata_json: "{}".to_string(),
    })?;

    for obs in observations {
        store.insert_observation_typed_for_run(obs, run_id)?;
    }

    let mut units_seen = 0usize;
    for obs in observations {
        if obs.kind() != ObservationKind::SystemdUnitSeen {
            continue;
        }
        let Some(unit) = obs.metadata().get("unit").and_then(|v| v.as_str()) else {
            continue;
        };
        let service_id = NodeId::service(unit);
        let existing = store.get_node_typed(&service_id)?;
        let node = GraphNode::service(unit, scan_time, existing.as_ref());
        store.upsert_node_typed(&node)?;
        units_seen += 1;
    }

    for ((from_unit, to_unit), obs_ids) in &dep_groups {
        let source_id = NodeId::service(from_unit);
        let target_id = NodeId::service(to_unit);
        for unit in [from_unit.as_str(), to_unit.as_str()] {
            let id = NodeId::service(unit);
            let existing = store.get_node_typed(&id)?;
            let node = GraphNode::service(unit, scan_time, existing.as_ref());
            store.upsert_node_typed(&node)?;
        }
        let Some(primary) = obs_ids.iter().find_map(|id| obs_by_id.get(id)) else {
            continue;
        };
        let unit_path = primary
            .metadata()
            .get("unit_path")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dep_key = primary
            .metadata()
            .get("key")
            .and_then(|v| v.as_str())
            .unwrap_or("Requires");
        let existing_edge =
            super::scan::load_existing_edge(store, &source_id, EdgeKind::DependsOn, &target_id)?;
        let edge = GraphEdge::observed_service_depends_on_declared(
            &source_id,
            &target_id,
            scan_time,
            existing_edge.as_ref(),
            dep_key,
            unit_path,
        );
        super::scan::upsert_edge_with_link(store, &edge, None)?;
        for obs_id in obs_ids {
            store.link_edge_observation(edge.id().as_str(), &obs_id.to_string(), "direct")?;
        }
        if seen_service_depends.insert((source_id.clone(), target_id.clone())) {
            *declared_depends_on_edge_count += 1;
        }
    }

    Ok(units_seen)
}

fn systemd_dependency_groups(
    observations: &[Observation],
) -> HashMap<(String, String), Vec<ObservationId>> {
    let mut map: HashMap<(String, String), Vec<ObservationId>> = HashMap::new();
    for obs in observations {
        if !matches!(
            obs.kind(),
            ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants
        ) {
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
