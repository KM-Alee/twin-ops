use std::collections::HashSet;

use twin_collectors::ProcessWarningKind;
use twin_core::{ChangeKind, GraphEdge, GraphNode};
use twin_store::{EdgeRow, NodeRow, Store, StoreError};

pub(crate) struct ScanHistorySession {
    seen_nodes: HashSet<String>,
    seen_edges: HashSet<String>,
    recorded_at_ns: i64,
    collector_run_id: Option<i64>,
    prefer_stale: bool,
    finalize_missing: bool,
}

impl ScanHistorySession {
    pub fn new(
        recorded_at_ns: i64,
        collector_run_id: Option<i64>,
        prefer_stale: bool,
        finalize_missing: bool,
    ) -> Self {
        Self {
            seen_nodes: HashSet::new(),
            seen_edges: HashSet::new(),
            recorded_at_ns,
            collector_run_id,
            prefer_stale,
            finalize_missing,
        }
    }

    pub fn prefer_stale_from_warnings(warnings: &[twin_collectors::ProcessWarning]) -> bool {
        let mut fd_permission = 0usize;
        let mut permission = 0usize;
        for warning in warnings {
            match warning.kind() {
                ProcessWarningKind::PermissionDenied => permission += 1,
                ProcessWarningKind::FdPermissionDenied => fd_permission += 1,
                _ => {}
            }
        }
        permission > 0 || fd_permission >= 50
    }

    pub fn finalize_missing_rows(&mut self, store: &mut Store) -> Result<(), StoreError> {
        if !self.finalize_missing {
            return Ok(());
        }
        let seen_nodes: Vec<String> = self.seen_nodes.iter().cloned().collect();
        let seen_edges: Vec<String> = self.seen_edges.iter().cloned().collect();
        store.mark_nodes_missing_since(
            &seen_nodes,
            self.recorded_at_ns,
            self.prefer_stale,
            self.collector_run_id,
        )?;
        store.mark_edges_missing_since(
            &seen_edges,
            self.recorded_at_ns,
            self.prefer_stale,
            self.collector_run_id,
        )?;
        Ok(())
    }
}

pub(crate) fn upsert_node(
    store: &mut Store,
    session: &mut ScanHistorySession,
    node: &GraphNode,
) -> Result<(), StoreError> {
    let row = NodeRow::from(node);
    let existing = store.get_node(node.id().as_str())?;
    if let Some(change_kind) = classify_node_change(existing.as_ref(), &row) {
        store.insert_node_history(
            &row,
            change_kind,
            session.recorded_at_ns,
            session.collector_run_id,
        )?;
    }
    store.upsert_node(&row)?;
    session.seen_nodes.insert(node.id().to_string());
    Ok(())
}

pub(crate) fn upsert_edge(
    store: &mut Store,
    session: &mut ScanHistorySession,
    edge: &GraphEdge,
) -> Result<(), StoreError> {
    let row = EdgeRow::from(edge);
    let existing = store.get_edge(edge.id().as_str())?;
    if let Some(change_kind) = classify_edge_change(existing.as_ref(), &row) {
        store.insert_edge_history(
            &row,
            change_kind,
            session.recorded_at_ns,
            session.collector_run_id,
        )?;
    }
    store.upsert_edge(&row)?;
    session.seen_edges.insert(edge.id().to_string());
    Ok(())
}

pub(crate) fn delete_edge(
    store: &mut Store,
    session: &mut ScanHistorySession,
    edge_id: &str,
) -> Result<(), StoreError> {
    if let Some(row) = store.get_edge(edge_id)? {
        let mut gone = row;
        gone.state = "gone".to_string();
        store.insert_edge_history(
            &gone,
            ChangeKind::Gone,
            session.recorded_at_ns,
            session.collector_run_id,
        )?;
    }
    store.delete_edge(edge_id)?;
    session.seen_edges.remove(edge_id);
    Ok(())
}

fn classify_node_change(existing: Option<&NodeRow>, node: &NodeRow) -> Option<ChangeKind> {
    let Some(prev) = existing else {
        return Some(ChangeKind::New);
    };
    if prev.state != "active" && node.state == "active" {
        return Some(ChangeKind::Reappeared);
    }
    if prev.label != node.label
        || prev.metadata_json != node.metadata_json
        || prev.kind != node.kind
        || prev.state != node.state
    {
        return Some(ChangeKind::Changed);
    }
    None
}

fn classify_edge_change(existing: Option<&EdgeRow>, edge: &EdgeRow) -> Option<ChangeKind> {
    let Some(prev) = existing else {
        return Some(ChangeKind::New);
    };
    if prev.state != "active" && edge.state == "active" {
        return Some(ChangeKind::Reappeared);
    }
    if prev.from_node_id != edge.from_node_id
        || prev.to_node_id != edge.to_node_id
        || prev.kind != edge.kind
        || prev.class != edge.class
        || prev.state != edge.state
        || prev.evidence_score != edge.evidence_score
        || prev.evidence_label != edge.evidence_label
        || prev.evidence_count != edge.evidence_count
        || prev.metadata_json != edge.metadata_json
    {
        return Some(ChangeKind::Changed);
    }
    None
}
