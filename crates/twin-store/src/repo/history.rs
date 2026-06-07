use rusqlite::{params, Row};

use twin_core::ChangeKind;

use crate::error::StoreError;
use crate::repo::edge::row_from_edge;
use crate::repo::node::row_from_node;
use crate::repo::observation::collect_rows;
use crate::repo::{EdgeRow, NodeRow};
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeHistoryRow {
    pub id: i64,
    pub node_id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub first_seen_ns: i64,
    pub last_seen_ns: i64,
    pub valid_from_ns: i64,
    pub valid_to_ns: Option<i64>,
    pub metadata_json: String,
    pub change_kind: String,
    pub recorded_at_ns: i64,
    pub collector_run_id: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeHistoryRow {
    pub id: i64,
    pub edge_id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub kind: String,
    pub class: String,
    pub state: String,
    pub evidence_score: i64,
    pub evidence_label: String,
    pub evidence_count: i64,
    pub first_seen_ns: i64,
    pub last_seen_ns: i64,
    pub metadata_json: String,
    pub change_kind: String,
    pub recorded_at_ns: i64,
    pub collector_run_id: Option<i64>,
}

impl Store {
    pub fn insert_node_history(
        &mut self,
        node: &NodeRow,
        change_kind: ChangeKind,
        recorded_at_ns: i64,
        collector_run_id: Option<i64>,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO node_history (
                    node_id, kind, label, state, first_seen_ns, last_seen_ns,
                    valid_from_ns, valid_to_ns, metadata_json, change_kind,
                    recorded_at_ns, collector_run_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    node.id,
                    node.kind,
                    node.label,
                    node.state,
                    node.first_seen_ns,
                    node.last_seen_ns,
                    node.valid_from_ns,
                    node.valid_to_ns,
                    node.metadata_json,
                    change_kind.as_str(),
                    recorded_at_ns,
                    collector_run_id,
                ],
            )
            .map_err(|source| StoreError::Upsert { source })?;
        Ok(())
    }

    pub fn insert_edge_history(
        &mut self,
        edge: &EdgeRow,
        change_kind: ChangeKind,
        recorded_at_ns: i64,
        collector_run_id: Option<i64>,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO edge_history (
                    edge_id, from_node_id, to_node_id, kind, class, state,
                    evidence_score, evidence_label, evidence_count,
                    first_seen_ns, last_seen_ns, metadata_json, change_kind,
                    recorded_at_ns, collector_run_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
                params![
                    edge.id,
                    edge.from_node_id,
                    edge.to_node_id,
                    edge.kind,
                    edge.class,
                    edge.state,
                    edge.evidence_score,
                    edge.evidence_label,
                    edge.evidence_count,
                    edge.first_seen_ns,
                    edge.last_seen_ns,
                    edge.metadata_json,
                    change_kind.as_str(),
                    recorded_at_ns,
                    collector_run_id,
                ],
            )
            .map_err(|source| StoreError::Upsert { source })?;
        Ok(())
    }

    pub fn list_node_history_since(
        &self,
        since_ns: i64,
    ) -> Result<Vec<NodeHistoryRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, node_id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json, change_kind,
                        recorded_at_ns, collector_run_id
                 FROM node_history WHERE recorded_at_ns >= ?1
                 ORDER BY recorded_at_ns, id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map(params![since_ns], row_from_node_history)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_edge_history_since(
        &self,
        since_ns: i64,
    ) -> Result<Vec<EdgeHistoryRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, edge_id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json, change_kind,
                        recorded_at_ns, collector_run_id
                 FROM edge_history WHERE recorded_at_ns >= ?1
                 ORDER BY recorded_at_ns, id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map(params![since_ns], row_from_edge_history)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_current_nodes(&self) -> Result<Vec<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM nodes WHERE state = 'active' ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], row_from_node)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_current_edges(&self) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE state = 'active' ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn mark_nodes_missing_since(
        &mut self,
        seen_ids: &[String],
        recorded_at_ns: i64,
        prefer_stale: bool,
        collector_run_id: Option<i64>,
    ) -> Result<usize, StoreError> {
        let active = self.list_active_node_ids()?;
        let seen: std::collections::HashSet<&str> = seen_ids.iter().map(String::as_str).collect();
        let mut count = 0usize;
        for id in active {
            if seen.contains(id.as_str()) {
                continue;
            }
            let Some(mut row) = self.get_node(&id)? else {
                continue;
            };
            let change_kind = if prefer_stale {
                ChangeKind::Stale
            } else {
                ChangeKind::Gone
            };
            row.state = if prefer_stale {
                "stale".to_string()
            } else {
                "gone".to_string()
            };
            row.valid_to_ns = Some(recorded_at_ns);
            self.upsert_node(&row)?;
            self.insert_node_history(&row, change_kind, recorded_at_ns, collector_run_id)?;
            count += 1;
        }
        Ok(count)
    }

    pub fn mark_edges_missing_since(
        &mut self,
        seen_ids: &[String],
        recorded_at_ns: i64,
        prefer_stale: bool,
        collector_run_id: Option<i64>,
    ) -> Result<usize, StoreError> {
        let active = self.list_active_edge_ids()?;
        let seen: std::collections::HashSet<&str> = seen_ids.iter().map(String::as_str).collect();
        let mut count = 0usize;
        for id in active {
            if seen.contains(id.as_str()) {
                continue;
            }
            let Some(mut row) = self.get_edge(&id)? else {
                continue;
            };
            let change_kind = if prefer_stale {
                ChangeKind::Stale
            } else {
                ChangeKind::Gone
            };
            row.state = if prefer_stale {
                "stale".to_string()
            } else {
                "gone".to_string()
            };
            self.upsert_edge(&row)?;
            self.insert_edge_history(&row, change_kind, recorded_at_ns, collector_run_id)?;
            count += 1;
        }
        Ok(count)
    }

    fn list_active_node_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM nodes WHERE state = 'active' ORDER BY id")
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    fn list_active_edge_ids(&self) -> Result<Vec<String>, StoreError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM edges WHERE state = 'active' ORDER BY id")
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }
}

fn row_from_node_history(row: &Row<'_>) -> Result<NodeHistoryRow, rusqlite::Error> {
    Ok(NodeHistoryRow {
        id: row.get(0)?,
        node_id: row.get(1)?,
        kind: row.get(2)?,
        label: row.get(3)?,
        state: row.get(4)?,
        first_seen_ns: row.get(5)?,
        last_seen_ns: row.get(6)?,
        valid_from_ns: row.get(7)?,
        valid_to_ns: row.get(8)?,
        metadata_json: row.get(9)?,
        change_kind: row.get(10)?,
        recorded_at_ns: row.get(11)?,
        collector_run_id: row.get(12)?,
    })
}

fn row_from_edge_history(row: &Row<'_>) -> Result<EdgeHistoryRow, rusqlite::Error> {
    Ok(EdgeHistoryRow {
        id: row.get(0)?,
        edge_id: row.get(1)?,
        from_node_id: row.get(2)?,
        to_node_id: row.get(3)?,
        kind: row.get(4)?,
        class: row.get(5)?,
        state: row.get(6)?,
        evidence_score: row.get(7)?,
        evidence_label: row.get(8)?,
        evidence_count: row.get(9)?,
        first_seen_ns: row.get(10)?,
        last_seen_ns: row.get(11)?,
        metadata_json: row.get(12)?,
        change_kind: row.get(13)?,
        recorded_at_ns: row.get(14)?,
        collector_run_id: row.get(15)?,
    })
}
