use std::collections::{HashMap, HashSet};

use twin_collectors::{SystemdUnitBatch, SYSTEMD_UNIT_COLLECTOR_NAME};
use twin_core::{GraphEdge, GraphNode, NodeId, ObservationId, TimestampNs};
use twin_observation::{Observation, ObservationKind};
use twin_store::{CollectorRunRow, Store, StoreError};

pub(crate) fn persist_systemd_in_scan(
    store: &mut Store,
    edge_cache: &mut super::scan::ScanEdgeCache,
    batch: &SystemdUnitBatch,
    observations: &[Observation],
    scan_time: TimestampNs,
    declared_depends_on_edge_count: &mut usize,
    seen_service_depends: &mut HashSet<(NodeId, NodeId)>,
) -> Result<usize, StoreError> {
    let dep_groups = super::scan_systemd_groups::group_unit_dep_observations(observations, |obs| {
        matches!(
            obs.kind(),
            ObservationKind::SystemdUnitRequires | ObservationKind::SystemdUnitWants
        )
    });
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
        super::scan_systemd_groups::persist_depends_on_group(
            store,
            edge_cache,
            &source_id,
            &target_id,
            super::scan_systemd_groups::EdgeObservationLink::AllIds(obs_ids),
            |existing| {
                GraphEdge::observed_service_depends_on_declared(
                    &source_id, &target_id, scan_time, existing, dep_key, unit_path,
                )
            },
        )?;
        if seen_service_depends.insert((source_id.clone(), target_id.clone())) {
            *declared_depends_on_edge_count += 1;
        }
    }

    Ok(units_seen)
}
