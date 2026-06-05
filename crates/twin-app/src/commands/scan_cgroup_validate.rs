use std::collections::HashSet;

use twin_collectors::ProcessRecord;
use twin_collectors::{try_connect_dbus, SystemdDBusReader};
use twin_core::{EdgeClass, EdgeKind, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_store::{Store, StoreError};

pub(crate) fn apply_cgroup_corrections(
    store: &mut Store,
    records: &[ProcessRecord],
    scan_time: TimestampNs,
    reader: Option<&dyn SystemdDBusReader>,
) -> Result<usize, StoreError> {
    let Some(reader) = reader else {
        return Ok(0);
    };
    let mut corrections = 0usize;
    let mut seen_paths: HashSet<String> = HashSet::new();
    for record in records {
        for membership in record.cgroup_memberships() {
            let Some(inferred) = &membership.service_unit else {
                continue;
            };
            let path = membership.path.clone();
            if !seen_paths.insert(path.clone()) {
                continue;
            }
            let dbus_unit = match reader.get_unit_by_control_group(&path) {
                Ok(Some(name)) => name,
                Ok(None) | Err(_) => continue,
            };
            if dbus_unit == *inferred {
                continue;
            }
            if !dbus_unit.ends_with(".service") && !dbus_unit.ends_with(".socket") {
                continue;
            }
            let corrected_id = NodeId::service(&dbus_unit);
            let inferred_id = NodeId::service(inferred);
            let process_id = NodeId::process(record.pid());
            let cgroup_id = NodeId::cgroup(&path);
            let existing = store.get_node_typed(&corrected_id)?;
            store.upsert_node_typed(&GraphNode::service(
                &dbus_unit,
                scan_time,
                existing.as_ref(),
            ))?;
            for target in [&process_id, &cgroup_id] {
                let edge = GraphEdge::inferred_service_owns(
                    &corrected_id,
                    target,
                    scan_time,
                    super::scan::load_existing_edge(store, &corrected_id, EdgeKind::Owns, target)?
                        .as_ref(),
                );
                super::scan::upsert_edge_with_link(store, &edge, None)?;
                if let Ok(Some(stale)) =
                    super::scan::load_existing_edge(store, &inferred_id, EdgeKind::Owns, target)
                {
                    if stale.class() == EdgeClass::Inferred {
                        store.delete_edge(stale.id().as_str())?;
                    }
                }
            }
            corrections += 1;
        }
    }
    Ok(corrections)
}

pub(crate) fn dbus_reader_for_scan() -> Option<Box<dyn SystemdDBusReader>> {
    if std::env::var("TWIN_SYSTEMD_UNIT_ROOT").is_ok()
        || std::env::var("TWIN_SYSTEMD_CGROUP_MAP").is_ok()
    {
        return None;
    }
    try_connect_dbus().map(|r| Box::new(r) as Box<dyn SystemdDBusReader>)
}
