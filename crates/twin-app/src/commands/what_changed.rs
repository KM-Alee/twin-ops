use std::collections::{BTreeSet, HashMap};
use std::str::FromStr;

use twin_core::{EdgeKind, NodeId, TimestampNs};
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
    crate::config_io::load(layout, request.config_override.as_deref())?;
    if !db_path.exists() {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(TemporalError::StoreOpen)?;
    if !store.is_initialized().map_err(TemporalError::Store)? {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }

    let now = TimestampNs::now();
    let since_ns = parse_since_duration(&request.since, now)?;
    if request.limit == 0 || request.limit > 10_000 || !(1..=4).contains(&request.max_depth) {
        return Err(TemporalError::InvalidQuery {
            reason: "limit must be 1–10000 and max_depth must be 1–4".into(),
        }
        .into());
    }
    let mut node_rows = store
        .list_node_history_since(since_ns)
        .map_err(TemporalError::Store)?;
    let mut edge_rows = store
        .list_edge_history_since(since_ns)
        .map_err(TemporalError::Store)?;

    let target = request
        .target_query
        .as_deref()
        .map(|query| {
            if query.contains(':') {
                NodeId::from_str(query).map_err(|source| AppError::InvalidGraphTarget {
                    value: query.into(),
                    source,
                })
            } else {
                super::resolve_service::resolve_service_target(
                    &store,
                    query,
                    super::resolve_service::ServiceNotFoundContext::Graph {
                        detail: String::new(),
                    },
                )
            }
        })
        .transpose()?;
    if let Some(target) = &target {
        if store
            .get_node_typed(target)
            .map_err(TemporalError::Store)?
            .is_none()
        {
            return Err(crate::GraphError::NodeNotFound { id: target.clone() }.into());
        }
        let mut links = store
            .list_unfinalized_edges()
            .map_err(TemporalError::Store)?
            .into_iter()
            .map(|edge| (edge.from_node_id, edge.to_node_id, edge.kind))
            .collect::<Vec<_>>();
        links.extend(edge_rows.iter().map(|edge| {
            (
                edge.from_node_id.clone(),
                edge.to_node_id.clone(),
                edge.kind.clone(),
            )
        }));
        let mut neighborhood = BTreeSet::from([target.to_string()]);
        let mut frontier = neighborhood.clone();
        for _ in 0..request.max_depth {
            let mut next = BTreeSet::new();
            for (from, to, kind) in &links {
                if !incident_link(kind) {
                    continue;
                }
                if frontier.contains(from) && !neighborhood.contains(to) {
                    next.insert(to.clone());
                }
                if frontier.contains(to) && !neighborhood.contains(from) {
                    next.insert(from.clone());
                }
            }
            neighborhood.extend(next.iter().cloned());
            frontier = next;
        }
        node_rows.retain(|row| neighborhood.contains(&row.node_id));
        edge_rows.retain(|row| {
            neighborhood.contains(&row.from_node_id) && neighborhood.contains(&row.to_node_id)
        });
    }
    let mut operational_changes =
        super::operational_changes::summarize(&store, &node_rows, &edge_rows, now)?;
    operational_changes.retain(|change| {
        request.categories.is_empty() || request.categories.contains(&change.category)
    });
    operational_changes.sort_by(|left, right| {
        left.recorded_at_ns
            .cmp(&right.recorded_at_ns)
            .then(left.entity.cmp(&right.entity))
            .then(left.category.cmp(&right.category))
    });
    let omitted_operational_changes = operational_changes.len().saturating_sub(request.limit);
    operational_changes.drain(..omitted_operational_changes);

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
                summary: store
                    .node_history_before(row)
                    .map_err(TemporalError::Store)?
                    .and_then(|before| super::diff::node_change_summary(&before, &row.into()))
                    .unwrap_or_else(|| "previous history unavailable".to_string()),
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
                summary: store
                    .edge_history_before(row)
                    .map_err(TemporalError::Store)?
                    .and_then(|before| super::diff::edge_change_summary(&before, &row.into()))
                    .unwrap_or_else(|| "previous history unavailable".to_string()),
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
        target: target.map(|id| id.to_string()),
        omitted_operational_changes,
        category_filter: request.categories.clone(),
        operational_changes,
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

fn incident_link(kind: &str) -> bool {
    matches!(
        EdgeKind::from_str(kind),
        Ok(EdgeKind::Owns
            | EdgeKind::ListensOn
            | EdgeKind::ConnectsTo
            | EdgeKind::ConfiguredBy
            | EdgeKind::DependsOn
            | EdgeKind::ProxiesTo
            | EdgeKind::References
            | EdgeKind::LoadsLibrary
            | EdgeKind::InstalledBy
            | EdgeKind::RunsImage
            | EdgeKind::MapsPort
            | EdgeKind::Selects
            | EdgeKind::RoutesTo
            | EdgeKind::UsesConfigMap
            | EdgeKind::UsesSecretRef
            | EdgeKind::UsesPvc)
    )
}

fn sort_nodes(nodes: &mut [WhatChangedNode]) {
    nodes.sort_by(|a, b| a.id.cmp(&b.id));
}

fn sort_edges(edges: &mut [WhatChangedEdge]) {
    edges.sort_by(|a, b| a.id.cmp(&b.id));
}
