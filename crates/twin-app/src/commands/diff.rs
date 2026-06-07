use std::collections::HashMap;

use twin_core::{parse_graph_ref, GraphRef};
use twin_store::{EdgeRow, NodeRow, Store};

use crate::error::{AppError, TemporalError};
use crate::model::{DiffEdge, DiffEdgeChange, DiffNode, DiffNodeChange, DiffResult};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::DiffRequest;

pub fn run_home(request: DiffRequest) -> Result<DiffResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    diff_at(&paths.layout, &request, &paths.db_path)
}

pub fn run(layout: &TwinLayout, request: &DiffRequest) -> Result<DiffResult, AppError> {
    diff_at(layout, request, &layout.db_file())
}

fn diff_at(
    layout: &TwinLayout,
    request: &DiffRequest,
    db_path: &std::path::Path,
) -> Result<DiffResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(TemporalError::StoreOpen)?;
    if !store.is_initialized().map_err(TemporalError::Store)? {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }

    let left_ref =
        parse_graph_ref(&request.left).map_err(|source| TemporalError::InvalidGraphRef {
            value: request.left.clone(),
            source,
        })?;
    let right_ref =
        parse_graph_ref(&request.right).map_err(|source| TemporalError::InvalidGraphRef {
            value: request.right.clone(),
            source,
        })?;

    let left_nodes = load_nodes(&store, &left_ref)?;
    let right_nodes = load_nodes(&store, &right_ref)?;
    let left_edges = load_edges(&store, &left_ref)?;
    let right_edges = load_edges(&store, &right_ref)?;

    let mut nodes_added = Vec::new();
    let mut nodes_removed = Vec::new();
    let mut nodes_changed = Vec::new();
    for (id, row) in &right_nodes {
        if !left_nodes.contains_key(id) {
            nodes_added.push(diff_node(row));
        } else if let Some(before) = left_nodes.get(id) {
            if let Some(summary) = node_change_summary(before, row) {
                nodes_changed.push(DiffNodeChange {
                    id: id.clone(),
                    label: row.label.clone(),
                    summary,
                });
            }
        }
    }
    for (id, row) in &left_nodes {
        if !right_nodes.contains_key(id) {
            nodes_removed.push(diff_node(row));
        }
    }

    let mut edges_added = Vec::new();
    let mut edges_removed = Vec::new();
    let mut edges_changed = Vec::new();
    for (id, row) in &right_edges {
        if !left_edges.contains_key(id) {
            edges_added.push(diff_edge(row));
        } else if let Some(before) = left_edges.get(id) {
            if let Some(summary) = edge_change_summary(before, row) {
                edges_changed.push(DiffEdgeChange {
                    id: id.clone(),
                    summary,
                });
            }
        }
    }
    for (id, row) in &left_edges {
        if !right_edges.contains_key(id) {
            edges_removed.push(diff_edge(row));
        }
    }

    nodes_added.sort_by(|a, b| a.id.cmp(&b.id));
    nodes_removed.sort_by(|a, b| a.id.cmp(&b.id));
    nodes_changed.sort_by(|a, b| a.id.cmp(&b.id));
    edges_added.sort_by(|a, b| a.id.cmp(&b.id));
    edges_removed.sort_by(|a, b| a.id.cmp(&b.id));
    edges_changed.sort_by(|a, b| a.id.cmp(&b.id));

    Ok(DiffResult {
        left_ref: request.left.clone(),
        right_ref: request.right.clone(),
        nodes_added,
        nodes_removed,
        nodes_changed,
        edges_added,
        edges_removed,
        edges_changed,
    })
}

fn load_nodes(
    store: &Store,
    graph_ref: &GraphRef,
) -> Result<HashMap<String, NodeRow>, TemporalError> {
    let rows = match graph_ref {
        GraphRef::Current => store.list_current_nodes().map_err(TemporalError::Store)?,
        GraphRef::Snapshot(id) => {
            if !store
                .snapshot_exists(id.as_str())
                .map_err(TemporalError::Store)?
            {
                return Err(TemporalError::SnapshotNotFound {
                    name: id.to_string(),
                });
            }
            store
                .load_snapshot_nodes(id.as_str())
                .map_err(TemporalError::Store)?
        }
    };
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

fn load_edges(
    store: &Store,
    graph_ref: &GraphRef,
) -> Result<HashMap<String, EdgeRow>, TemporalError> {
    let rows = match graph_ref {
        GraphRef::Current => store.list_current_edges().map_err(TemporalError::Store)?,
        GraphRef::Snapshot(id) => {
            if !store
                .snapshot_exists(id.as_str())
                .map_err(TemporalError::Store)?
            {
                return Err(TemporalError::SnapshotNotFound {
                    name: id.to_string(),
                });
            }
            store
                .load_snapshot_edges(id.as_str())
                .map_err(TemporalError::Store)?
        }
    };
    Ok(rows.into_iter().map(|row| (row.id.clone(), row)).collect())
}

fn diff_node(row: &NodeRow) -> DiffNode {
    DiffNode {
        id: row.id.clone(),
        label: row.label.clone(),
        kind: row.kind.clone(),
        state: row.state.clone(),
    }
}

fn diff_edge(row: &EdgeRow) -> DiffEdge {
    DiffEdge {
        id: row.id.clone(),
        from_node_id: row.from_node_id.clone(),
        to_node_id: row.to_node_id.clone(),
        kind: row.kind.clone(),
        class: row.class.clone(),
        state: row.state.clone(),
    }
}

fn node_change_summary(before: &NodeRow, after: &NodeRow) -> Option<String> {
    let mut parts = Vec::new();
    if before.label != after.label {
        parts.push(format!("label {} -> {}", before.label, after.label));
    }
    if before.state != after.state {
        parts.push(format!("state {} -> {}", before.state, after.state));
    }
    if before.metadata_json != after.metadata_json {
        parts.push("metadata changed".to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

fn edge_change_summary(before: &EdgeRow, after: &EdgeRow) -> Option<String> {
    let mut parts = Vec::new();
    if before.state != after.state {
        parts.push(format!("state {} -> {}", before.state, after.state));
    }
    if before.class != after.class {
        parts.push(format!("class {} -> {}", before.class, after.class));
    }
    if before.evidence_label != after.evidence_label
        || before.evidence_count != after.evidence_count
    {
        parts.push(format!(
            "evidence {} ({}) -> {} ({})",
            before.evidence_label,
            before.evidence_count,
            after.evidence_label,
            after.evidence_count
        ));
    }
    if before.metadata_json != after.metadata_json {
        parts.push("metadata changed".to_string());
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}
