use std::collections::HashMap;

use twin_core::TimestampNs;
use twin_store::Store;

use crate::commands::duration::parse_since_duration;
use crate::commands::scan_quality::assess_scan_quality;
use crate::error::{AppError, TemporalError};
use crate::model::{
    WhatChangedEdge, WhatChangedEdgeDelta, WhatChangedNode, WhatChangedNodeDelta, WhatChangedResult,
};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::WhatChangedRequest;

pub fn run_home(request: WhatChangedRequest) -> Result<WhatChangedResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    what_changed_at(&paths.layout, &request, &paths.db_path)
}

pub fn run(
    layout: &TwinLayout,
    request: &WhatChangedRequest,
) -> Result<WhatChangedResult, AppError> {
    what_changed_at(layout, request, &layout.db_file())
}

fn what_changed_at(
    layout: &TwinLayout,
    request: &WhatChangedRequest,
    db_path: &std::path::Path,
) -> Result<WhatChangedResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(TemporalError::StoreOpen)?;
    if !store.is_initialized().map_err(TemporalError::Store)? {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }

    let now = TimestampNs::now();
    let since_ns = parse_since_duration(&request.since, now)?;
    let node_rows = store
        .list_node_history_since(since_ns)
        .map_err(TemporalError::Store)?;
    let edge_rows = store
        .list_edge_history_since(since_ns)
        .map_err(TemporalError::Store)?;

    let mut latest_node: HashMap<String, _> = HashMap::new();
    for row in node_rows {
        latest_node.insert(row.node_id.clone(), row);
    }
    let mut latest_edge: HashMap<String, _> = HashMap::new();
    for row in edge_rows {
        latest_edge.insert(row.edge_id.clone(), row);
    }

    let mut new_nodes = Vec::new();
    let mut disappeared_nodes = Vec::new();
    let mut stale_nodes = Vec::new();
    let mut changed_nodes = Vec::new();
    let mut reappeared_nodes = Vec::new();
    for row in latest_node.values() {
        let entry = WhatChangedNode {
            id: row.node_id.clone(),
            label: row.label.clone(),
            kind: row.kind.clone(),
            state: row.state.clone(),
        };
        match row.change_kind.as_str() {
            "new" => new_nodes.push(entry),
            "gone" => disappeared_nodes.push(entry),
            "stale" => stale_nodes.push(entry),
            "reappeared" => reappeared_nodes.push(entry),
            "changed" => changed_nodes.push(WhatChangedNodeDelta {
                id: row.node_id.clone(),
                label: row.label.clone(),
                kind: row.kind.clone(),
                summary: format!("state {}", row.state),
            }),
            _ => {}
        }
    }

    let mut new_edges = Vec::new();
    let mut disappeared_edges = Vec::new();
    let mut stale_edges = Vec::new();
    let mut changed_edges = Vec::new();
    let mut reappeared_edges = Vec::new();
    for row in latest_edge.values() {
        let entry = WhatChangedEdge {
            id: row.edge_id.clone(),
            from_node_id: row.from_node_id.clone(),
            to_node_id: row.to_node_id.clone(),
            kind: row.kind.clone(),
            class: row.class.clone(),
            state: row.state.clone(),
        };
        match row.change_kind.as_str() {
            "new" => new_edges.push(entry),
            "gone" => disappeared_edges.push(entry),
            "stale" => stale_edges.push(entry),
            "reappeared" => reappeared_edges.push(entry),
            "changed" => changed_edges.push(WhatChangedEdgeDelta {
                id: row.edge_id.clone(),
                summary: format!("state {}", row.state),
            }),
            _ => {}
        }
    }

    sort_nodes(&mut new_nodes);
    sort_nodes(&mut disappeared_nodes);
    sort_nodes(&mut stale_nodes);
    sort_nodes(&mut reappeared_nodes);
    changed_nodes.sort_by(|a, b| a.id.cmp(&b.id));
    sort_edges(&mut new_edges);
    sort_edges(&mut disappeared_edges);
    sort_edges(&mut stale_edges);
    sort_edges(&mut reappeared_edges);
    changed_edges.sort_by(|a, b| a.id.cmp(&b.id));

    let mut unknowns = Vec::new();
    if let Ok(quality) = assess_scan_quality(&store) {
        if !quality.impact_reliable {
            unknowns.push(
                "scan coverage degraded — disappearance claims may be incomplete".to_string(),
            );
        }
        unknowns.extend(quality.reasons);
    }

    Ok(WhatChangedResult {
        since_ns,
        since_label: request.since.clone(),
        generated_at_ns: now.as_i64(),
        new_nodes,
        new_edges,
        disappeared_nodes,
        disappeared_edges,
        stale_nodes,
        stale_edges,
        changed_nodes,
        changed_edges,
        reappeared_nodes,
        reappeared_edges,
        unknowns,
    })
}

fn sort_nodes(nodes: &mut [WhatChangedNode]) {
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
}

fn sort_edges(edges: &mut [WhatChangedEdge]) {
    edges.sort_by(|a, b| a.id.cmp(&b.id));
}
