use std::collections::BTreeSet;
use twin_core::{NodeId, NodeKind};
use twin_store::{Store, StoreError};

pub(crate) fn ownership_conflict(store: &Store, process: &NodeId) -> Result<bool, StoreError> {
    if process.kind() != Some(NodeKind::Process) {
        return Ok(false);
    }
    let owners: BTreeSet<_> = store
        .list_active_edges_to(process.as_str())?
        .into_iter()
        .filter(|edge| edge.kind == "owns" && edge.from_node_id.starts_with("service:"))
        .map(|edge| edge.from_node_id)
        .collect();
    Ok(owners.len() > 1)
}

pub(crate) fn observations_conflict(store: &Store, ids: &[String]) -> Result<bool, StoreError> {
    let mut processes = BTreeSet::new();
    let ids: Vec<_> = ids.iter().map(String::as_str).collect();
    for observation in store.get_observations_by_ids(&ids)?.values() {
        for id in [
            observation.subject_node_id.as_deref(),
            observation.object_node_id.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if let Ok(id) = id.parse::<NodeId>() {
                if id.kind() == Some(NodeKind::Process) {
                    processes.insert(id);
                }
            }
        }
        if let Ok(metadata) = serde_json::from_str::<serde_json::Value>(&observation.metadata_json)
        {
            if let Some(pids) = metadata.get("owner_pids").and_then(|pids| pids.as_array()) {
                for pid in pids {
                    if let Some(pid) = pid.as_str().and_then(|pid| pid.parse::<u32>().ok()) {
                        processes.insert(NodeId::process(pid));
                    }
                }
            }
        }
    }
    for process in processes {
        if ownership_conflict(store, &process)? {
            return Ok(true);
        }
    }
    Ok(false)
}
