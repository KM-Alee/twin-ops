use std::collections::{HashMap, HashSet};

use twin_collectors::ProcessWarningKind;
use twin_core::{ChangeKind, GraphEdge, GraphNode, TimestampNs};
use twin_store::{EdgeRow, NodeRow, Store, StoreError};

/// The state an entity had before this pass and the history row this pass wrote for it.
struct PassWrite<T> {
    before: Option<T>,
    history_id: Option<i64>,
}

pub(crate) struct ScanHistorySession {
    seen_nodes: HashSet<String>,
    seen_edges: HashSet<String>,
    node_writes: HashMap<String, PassWrite<NodeRow>>,
    edge_writes: HashMap<String, PassWrite<EdgeRow>>,
    recorded_at_ns: i64,
    collector_run_id: Option<i64>,
    prefer_stale: bool,
    finalize_missing: bool,
    scope: FinalizeScope,
    skipped_sources: HashSet<String>,
    uncertain_sources: HashSet<String>,
    complete_sources: HashSet<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FinalizeScope {
    /// Host rows (processes, services, ports, files, mounts, libraries, packages,
    /// containers). Kubernetes-sourced rows belong to `twin k8s scan`.
    Host,
    /// Kubernetes-sourced rows only.
    K8s,
}

impl ScanHistorySession {
    pub fn new(
        recorded_at_ns: i64,
        collector_run_id: Option<i64>,
        prefer_stale: bool,
        finalize_missing: bool,
        scope: FinalizeScope,
    ) -> Self {
        Self {
            seen_nodes: HashSet::new(),
            seen_edges: HashSet::new(),
            node_writes: HashMap::new(),
            edge_writes: HashMap::new(),
            recorded_at_ns,
            collector_run_id,
            prefer_stale,
            finalize_missing,
            scope,
            skipped_sources: HashSet::new(),
            uncertain_sources: HashSet::new(),
            complete_sources: HashSet::new(),
        }
    }

    pub fn collector_run_id(&self) -> Option<i64> {
        self.collector_run_id
    }

    pub fn recorded_at(&self) -> TimestampNs {
        TimestampNs::new(self.recorded_at_ns)
    }

    pub fn prefer_stale_from_warnings(warnings: &[twin_collectors::ProcessWarning]) -> bool {
        warnings.iter().any(|warning| {
            matches!(
                warning.kind(),
                ProcessWarningKind::PermissionDenied
                    | ProcessWarningKind::Malformed
                    | ProcessWarningKind::FdPermissionDenied
                    | ProcessWarningKind::FdMalformed
                    | ProcessWarningKind::FdVanished
                    | ProcessWarningKind::CgroupPermissionDenied
                    | ProcessWarningKind::CgroupMalformed
            )
        })
    }

    pub fn skip_source(&mut self, source: &str) {
        self.skipped_sources.insert(source.to_string());
    }

    pub fn complete_source(&mut self, source: &str) {
        self.complete_sources.insert(source.into());
    }

    fn source_prefers_stale(&self, source: Option<String>) -> bool {
        source
            .as_ref()
            .is_some_and(|source| self.uncertain_sources.contains(source))
            || (self.prefer_stale
                && source
                    .as_ref()
                    .is_none_or(|source| !self.complete_sources.contains(source)))
    }

    /// Whether some writer already recorded this edge in the current pass.
    pub fn has_seen_edge(&self, edge_id: &str) -> bool {
        self.seen_edges.contains(edge_id)
    }

    pub fn uncertain_source(&mut self, source: &str) {
        self.uncertain_sources.insert(source.to_string());
    }

    pub fn finalize_missing_rows(&mut self, store: &mut Store) -> Result<(), StoreError> {
        if !self.finalize_missing {
            return Ok(());
        }
        let seen_nodes: HashSet<&str> = self.seen_nodes.iter().map(String::as_str).collect();
        let active_nodes = store.list_unfinalized_nodes()?;
        let active_edges = store.list_unfinalized_edges()?;
        let uncertain_nodes: HashSet<&str> = active_edges
            .iter()
            .filter(|row| {
                edge_source(row).is_some_and(|source| self.uncertain_sources.contains(&source))
            })
            .flat_map(|row| [row.from_node_id.as_str(), row.to_node_id.as_str()])
            .collect();
        let missing_nodes: Vec<String> = active_nodes
            .iter()
            .filter(|row| self.node_in_scope(row) && !seen_nodes.contains(row.id.as_str()))
            .map(|row| row.id.clone())
            .collect();
        let stale_nodes: HashSet<&str> = active_nodes
            .iter()
            .filter(|row| self.source_prefers_stale(metadata_source(&row.metadata_json)))
            .map(|row| row.id.as_str())
            .collect();
        let (uncertain, confirmed): (Vec<_>, Vec<_>) =
            missing_nodes.iter().cloned().partition(|id| {
                uncertain_nodes.contains(id.as_str()) || stale_nodes.contains(id.as_str())
            });
        store.mark_nodes_missing(
            &confirmed,
            self.recorded_at_ns,
            false,
            self.collector_run_id,
        )?;
        store.mark_nodes_missing(&uncertain, self.recorded_at_ns, true, self.collector_run_id)?;

        let missing_set: HashSet<&str> = missing_nodes.iter().map(String::as_str).collect();
        let still_active: HashSet<&str> = active_nodes
            .iter()
            .map(|row| row.id.as_str())
            .filter(|id| !missing_set.contains(id))
            .collect();
        let unavailable_nodes: HashSet<&str> = active_nodes
            .iter()
            .filter(|row| {
                metadata_source(&row.metadata_json)
                    .is_some_and(|source| self.skipped_sources.contains(&source))
            })
            .map(|row| row.id.as_str())
            .collect();
        let seen_edges: HashSet<&str> = self.seen_edges.iter().map(String::as_str).collect();
        let missing_edges: Vec<String> = active_edges
            .iter()
            .filter(|row| !seen_edges.contains(row.id.as_str()))
            .filter(|row| {
                !unavailable_nodes.contains(row.from_node_id.as_str())
                    && !unavailable_nodes.contains(row.to_node_id.as_str())
            })
            .filter(|row| self.edge_in_scope(row, &still_active))
            .map(|row| row.id.clone())
            .collect();
        let uncertain_edge_ids: HashSet<&str> = active_edges
            .iter()
            .filter(|row| {
                edge_source(row).is_some_and(|source| self.uncertain_sources.contains(&source))
            })
            .map(|row| row.id.as_str())
            .collect();
        let stale_edges: HashSet<&str> = active_edges
            .iter()
            .filter(|row| self.source_prefers_stale(edge_source(row)))
            .map(|row| row.id.as_str())
            .collect();
        let (uncertain, confirmed): (Vec<_>, Vec<_>) = missing_edges.into_iter().partition(|id| {
            uncertain_edge_ids.contains(id.as_str()) || stale_edges.contains(id.as_str())
        });
        store.mark_edges_missing(
            &confirmed,
            self.recorded_at_ns,
            false,
            self.collector_run_id,
        )?;
        store.mark_edges_missing(&uncertain, self.recorded_at_ns, true, self.collector_run_id)?;
        Ok(())
    }

    fn node_in_scope(&self, row: &NodeRow) -> bool {
        match self.scope {
            FinalizeScope::Host => match metadata_source(&row.metadata_json) {
                Some(source) => source != "k8s" && !self.skipped_sources.contains(&source),
                None => true,
            },
            FinalizeScope::K8s => metadata_source(&row.metadata_json).as_deref() == Some("k8s"),
        }
    }

    fn edge_in_scope(&self, row: &EdgeRow, still_active: &HashSet<&str>) -> bool {
        let source = edge_source(row);
        match self.scope {
            FinalizeScope::Host => match source.as_deref() {
                Some("k8s") => false,
                Some(source) if self.skipped_sources.contains(source) => false,
                // Stored eBPF connect evidence outlives the connection; the edge
                // is kept while its source endpoint is still known.
                Some("ebpf_tcp") => !still_active.contains(row.from_node_id.as_str()),
                _ => true,
            },
            FinalizeScope::K8s => source.as_deref() == Some("k8s"),
        }
    }
}

fn edge_source(row: &EdgeRow) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(&row.metadata_json).ok()?;
    let source = metadata_source(&row.metadata_json);
    let socket = matches!(
        source.as_deref(),
        Some(
            "proc_socket_inode_join"
                | "proc_tcp_established_inode_join"
                | "proc_udp_connected_inode_join"
        )
    ) || matches!(
        value.get("inference").and_then(|value| value.as_str()),
        Some(
            "service_owns_listening_process"
                | "service_owns_connected_process"
                | "active_connection_to_listening_service"
                | "unix_connection_to_listener_service"
        )
    );
    if socket {
        return Some(
            if row.to_node_id.starts_with("port:udp:")
                || source.as_deref() == Some("proc_udp_connected_inode_join")
            {
                "proc_net_udp"
            } else if row.to_node_id.starts_with("unix:")
                || value.get("inference").and_then(|value| value.as_str())
                    == Some("unix_connection_to_listener_service")
            {
                "proc_net_unix"
            } else {
                "proc_net_tcp"
            }
            .into(),
        );
    }
    source
}

fn metadata_source(metadata_json: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(metadata_json).ok()?;
    value
        .get("source")
        .and_then(|source| source.as_str())
        .map(str::to_string)
        .or_else(|| {
            (value.get("inference").and_then(|value| value.as_str()) == Some("nginx_proxy_pass")
                || value.get("proxy_pass").is_some())
            .then(|| "nginx".to_string())
        })
}

/// Several writers may touch one entity in a pass; history records the net change of the
/// pass once, and nothing when the entity ends the pass as it started.
pub(crate) fn upsert_node(
    store: &mut Store,
    session: &mut ScanHistorySession,
    node: &GraphNode,
) -> Result<(), StoreError> {
    let row = NodeRow::from(node);
    let write = match session.node_writes.entry(row.id.clone()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(PassWrite {
            before: store.get_node(&row.id)?,
            history_id: None,
        }),
    };
    if let Some(previous) = write.history_id.take() {
        store.delete_node_history(previous)?;
    }
    if let Some(change_kind) = classify_node_change(write.before.as_ref(), &row) {
        write.history_id = Some(store.insert_node_history(
            &row,
            change_kind,
            session.recorded_at_ns,
            session.collector_run_id,
        )?);
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
    let write = match session.edge_writes.entry(row.id.clone()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(PassWrite {
            before: store.get_edge(&row.id)?,
            history_id: None,
        }),
    };
    if let Some(previous) = write.history_id.take() {
        store.delete_edge_history(previous)?;
    }
    if let Some(change_kind) = classify_edge_change(write.before.as_ref(), &row) {
        write.history_id = Some(store.insert_edge_history(
            &row,
            change_kind,
            session.recorded_at_ns,
            session.collector_run_id,
        )?);
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
    let write = match session.edge_writes.entry(edge_id.to_string()) {
        std::collections::hash_map::Entry::Occupied(entry) => entry.into_mut(),
        std::collections::hash_map::Entry::Vacant(entry) => entry.insert(PassWrite {
            before: store.get_edge(edge_id)?,
            history_id: None,
        }),
    };
    if let Some(previous) = write.history_id.take() {
        store.delete_edge_history(previous)?;
    }
    if let Some(row) = write.before.as_ref().filter(|row| row.state != "gone") {
        let mut gone = row.clone();
        gone.state = "gone".to_string();
        write.history_id = Some(store.insert_edge_history(
            &gone,
            ChangeKind::Gone,
            session.recorded_at_ns,
            session.collector_run_id,
        )?);
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
        || stable_metadata(&prev.metadata_json) != stable_metadata(&node.metadata_json)
        || prev.kind != node.kind
        || prev.state != node.state
    {
        return Some(ChangeKind::Changed);
    }
    None
}

/// Keys restamped on every sighting of an unchanged node.
const PER_SIGHTING_NODE_KEYS: [&str; 1] = ["runtime_observed_at_ns"];

fn stable_metadata(metadata_json: &str) -> Option<serde_json::Value> {
    let mut value: serde_json::Value = serde_json::from_str(metadata_json).ok()?;
    if let Some(map) = value.as_object_mut() {
        for key in PER_SIGHTING_NODE_KEYS {
            map.remove(key);
        }
    }
    Some(value)
}

/// `evidence_count` and `last_seen_ns` move on every sighting of an unchanged edge; they
/// are not changes and must not add a history row per scan.
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
        || prev.metadata_json != edge.metadata_json
    {
        return Some(ChangeKind::Changed);
    }
    None
}
