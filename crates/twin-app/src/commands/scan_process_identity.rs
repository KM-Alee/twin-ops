use twin_collectors::ProcessRecord;
use twin_core::{
    GraphEdge, GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState, TimestampNs,
};
use twin_store::{Store, StoreError};

use super::scan_history::ScanHistorySession;

pub(super) fn process_node(
    store: &mut Store,
    history: &ScanHistorySession,
    record: &ProcessRecord,
    now: TimestampNs,
) -> Result<GraphNode, StoreError> {
    let id = NodeId::process(record.pid());
    let existing = store.get_node_typed(&id)?;
    let previous: serde_json::Value = existing
        .as_ref()
        .and_then(|node| serde_json::from_str(node.metadata().as_str()).ok())
        .unwrap_or_default();
    let changed_start = previous
        .get("start_ticks")
        .and_then(|value| value.as_u64())
        .zip(record.start_ticks())
        .is_some_and(|(old, new)| old != new);
    let changed_boot = previous
        .get("boot_id")
        .and_then(|value| value.as_str())
        .zip(record.boot_id())
        .is_some_and(|(old, new)| old != new);
    let changed = changed_start
        || changed_boot
        || existing
            .as_ref()
            .is_some_and(|node| node.state() == NodeState::Gone);
    if changed {
        let mut edges = store.list_edges_from(id.as_str())?;
        edges.extend(store.list_edges_to(id.as_str())?);
        let ids: Vec<String> = edges
            .iter()
            .filter(|row| row.state != "gone")
            .map(|row| row.id.clone())
            .collect();
        store.mark_edges_missing(&ids, now.as_i64(), false, history.collector_run_id())?;
        store.mark_nodes_missing(
            &[id.to_string()],
            now.as_i64(),
            false,
            history.collector_run_id(),
        )?;
        for row in edges {
            let edge = GraphEdge::try_from(&row)?;
            store.reset_edge_incarnation(edge.id(), now)?;
        }
        store.reset_node_incarnation(&id, now)?;
    }
    let prior = existing.as_ref().filter(|_| !changed);
    let start_ticks = record.start_ticks().or_else(|| {
        if changed {
            None
        } else {
            previous.get("start_ticks").and_then(|value| value.as_u64())
        }
    });
    let boot_id = record
        .boot_id()
        .or_else(|| previous.get("boot_id").and_then(|value| value.as_str()));
    let metadata = serde_json::json!({"start_ticks": start_ticks, "boot_id": boot_id, "start_identity_known": record.start_ticks().is_some(), "boot_identity_known": record.boot_id().is_some()});
    Ok(GraphNode::from_parts(GraphNodeParts {
        id,
        kind: NodeKind::Process,
        label: record.label(),
        state: NodeState::Active,
        first_seen: prior.map_or(now, GraphNode::first_seen),
        last_seen: now,
        valid_from: prior.map_or(now, GraphNode::valid_from),
        valid_to: None,
        metadata: GraphMetadata::from_json(metadata.to_string()),
    }))
}
