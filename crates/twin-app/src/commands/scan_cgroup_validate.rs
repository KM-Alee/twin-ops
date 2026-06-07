use std::collections::HashSet;

use twin_collectors::ProcessRecord;
use twin_collectors::{try_connect_dbus, SystemdDBusReader};
use twin_core::{EdgeClass, EdgeKind, GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_store::{Store, StoreError};

struct CgroupUnitLookup {
    by_prefix: Vec<(String, String)>,
}

impl CgroupUnitLookup {
    fn from_reader(
        reader: &dyn SystemdDBusReader,
    ) -> Result<Self, twin_collectors::SystemdDBusError> {
        let units = reader.list_units()?;
        let mut by_prefix: Vec<(String, String)> = units
            .into_iter()
            .filter(|u| !u.control_group.is_empty())
            .map(|u| (u.control_group, u.name))
            .collect();
        by_prefix.sort_by(|a, b| b.0.len().cmp(&a.0.len()));
        Ok(Self { by_prefix })
    }

    fn resolve(&self, cgroup_path: &str) -> Option<&str> {
        for (prefix, unit) in &self.by_prefix {
            if cgroup_path == prefix.as_str() || cgroup_path.starts_with(&format!("{prefix}/")) {
                return Some(unit.as_str());
            }
        }
        None
    }
}

pub(crate) fn apply_cgroup_corrections(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut super::scan::ScanEdgeCache,
    records: &[ProcessRecord],
    scan_time: TimestampNs,
    reader: Option<&dyn SystemdDBusReader>,
) -> Result<usize, StoreError> {
    let Some(reader) = reader else {
        return Ok(0);
    };
    let cgroup_lookup = CgroupUnitLookup::from_reader(reader).ok();
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
            let dbus_unit = cgroup_lookup
                .as_ref()
                .and_then(|map| map.resolve(&path).map(str::to_string))
                .or_else(|| reader.get_unit_by_control_group(&path).ok().flatten());
            let Some(dbus_unit) = dbus_unit else {
                continue;
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
            super::scan_history::upsert_node(
                store,
                history,
                &GraphNode::service(&dbus_unit, scan_time, existing.as_ref()),
            )?;
            for target in [&process_id, &cgroup_id] {
                let edge = GraphEdge::inferred_service_owns(
                    &corrected_id,
                    target,
                    scan_time,
                    super::scan::load_existing_edge(
                        store,
                        edge_cache,
                        &corrected_id,
                        EdgeKind::Owns,
                        target,
                    )?
                    .as_ref(),
                );
                super::scan::upsert_edge_with_link(store, history, &edge, None)?;
                if let Ok(Some(stale)) = super::scan::load_existing_edge(
                    store,
                    edge_cache,
                    &inferred_id,
                    EdgeKind::Owns,
                    target,
                ) {
                    if stale.class() == EdgeClass::Inferred {
                        super::scan_history::delete_edge(store, history, stale.id().as_str())?;
                    }
                }
            }
            corrections += 1;
        }
    }
    Ok(corrections)
}

pub(crate) fn dbus_reader_for_scan() -> Option<Box<dyn SystemdDBusReader>> {
    if twin_collectors::should_skip_live_dbus() {
        return None;
    }
    try_connect_dbus().map(|r| Box::new(r) as Box<dyn SystemdDBusReader>)
}
