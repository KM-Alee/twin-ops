use twin_core::{EdgeId, EdgeKind, GraphEdge, NodeId, NodeKind};
use twin_observation::Observation;
use twin_store::{Store, StoreError};

use super::scan_history::{self, ScanHistorySession};

pub(crate) fn kernel_id(path: &str) -> Option<u64> {
    use std::os::unix::fs::MetadataExt;
    let root = super::scan_config_files::host_path_for_discovery("/sys/fs/cgroup");
    if !root.join("cgroup.controllers").is_file()
        || !std::path::Path::new(path).components().all(|part| {
            matches!(
                part,
                std::path::Component::RootDir | std::path::Component::Normal(_)
            )
        })
    {
        return None;
    }
    let metadata = std::fs::symlink_metadata(root.join(path.trim_start_matches('/'))).ok()?;
    metadata.is_dir().then(|| metadata.ino())
}

pub(crate) fn attribute(
    store: &mut Store,
    history: &mut ScanHistorySession,
    observation: &Observation,
    pid: u32,
    kernel_id: u64,
) -> Result<(), StoreError> {
    if kernel_id == 0 {
        return Ok(());
    }
    let mut matches = Vec::new();
    for node in store.list_nodes_by_kind_typed(NodeKind::Cgroup)? {
        if node.state() != twin_core::NodeState::Active {
            continue;
        }
        let metadata: serde_json::Value =
            serde_json::from_str(node.metadata().as_str()).unwrap_or_default();
        if metadata
            .get("kernel_cgroup_id")
            .and_then(|id| id.as_str())
            .and_then(|id| id.parse::<u64>().ok())
            == Some(kernel_id)
        {
            matches.push(node.id().clone());
        }
    }
    let [cgroup] = matches.as_slice() else {
        return Ok(());
    };
    let process = NodeId::process(pid);
    let edge_id = EdgeId::new(&process, EdgeKind::InCgroup, cgroup);
    let existing = store
        .get_edge(edge_id.as_str())?
        .as_ref()
        .map(GraphEdge::try_from)
        .transpose()?;
    let edge =
        GraphEdge::observed_in_cgroup(&process, cgroup, observation.timestamp(), existing.as_ref());
    scan_history::upsert_edge(store, history, &edge)?;
    store.link_edge_observation(edge.id().as_str(), &observation.id().to_string(), "direct")?;
    for owner in store.list_active_edges_to(cgroup.as_str())? {
        if owner.kind != "owns" {
            continue;
        }
        let Ok(service) = owner.from_node_id.parse::<NodeId>() else {
            continue;
        };
        if service.kind() != Some(NodeKind::Service) {
            continue;
        }
        let edge_id = EdgeId::new(&service, EdgeKind::Owns, &process);
        let existing = store
            .get_edge(edge_id.as_str())?
            .as_ref()
            .map(GraphEdge::try_from)
            .transpose()?;
        let edge = GraphEdge::inferred_service_owns_process(
            &service,
            &process,
            observation.timestamp(),
            existing.as_ref(),
        );
        scan_history::upsert_edge(store, history, &edge)?;
        store.link_edge_observation(
            edge.id().as_str(),
            &observation.id().to_string(),
            "support",
        )?;
    }
    Ok(())
}
