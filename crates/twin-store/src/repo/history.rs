use rusqlite::{params, OptionalExtension, Row};

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
    ) -> Result<i64, StoreError> {
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
        Ok(self.conn.last_insert_rowid())
    }

    pub fn insert_edge_history(
        &mut self,
        edge: &EdgeRow,
        change_kind: ChangeKind,
        recorded_at_ns: i64,
        collector_run_id: Option<i64>,
    ) -> Result<i64, StoreError> {
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
        Ok(self.conn.last_insert_rowid())
    }

    pub fn delete_node_history(&mut self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM node_history WHERE id = ?1", params![id])
            .map_err(|source| StoreError::Upsert { source })?;
        Ok(())
    }

    pub fn delete_edge_history(&mut self, id: i64) -> Result<(), StoreError> {
        self.conn
            .execute("DELETE FROM edge_history WHERE id = ?1", params![id])
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

    pub fn node_history_before(&self, row: &NodeHistoryRow) -> Result<Option<NodeRow>, StoreError> {
        self.conn
            .query_row(
                "SELECT node_id, kind, label, state, first_seen_ns, last_seen_ns,
                    valid_from_ns, valid_to_ns, metadata_json
             FROM node_history WHERE node_id = ?1
               AND (recorded_at_ns < ?2 OR (recorded_at_ns = ?2 AND id < ?3))
             ORDER BY recorded_at_ns DESC, id DESC LIMIT 1",
                params![row.node_id, row.recorded_at_ns, row.id],
                row_from_node,
            )
            .optional()
            .map_err(|source| StoreError::Query { source })
    }

    pub fn edge_history_before(&self, row: &EdgeHistoryRow) -> Result<Option<EdgeRow>, StoreError> {
        self.conn
            .query_row(
                "SELECT edge_id, from_node_id, to_node_id, kind, class, state,
                    evidence_score, evidence_label, evidence_count,
                    first_seen_ns, last_seen_ns, metadata_json
             FROM edge_history WHERE edge_id = ?1
               AND (recorded_at_ns < ?2 OR (recorded_at_ns = ?2 AND id < ?3))
             ORDER BY recorded_at_ns DESC, id DESC LIMIT 1",
                params![row.edge_id, row.recorded_at_ns, row.id],
                row_from_edge,
            )
            .optional()
            .map_err(|source| StoreError::Query { source })
    }

    pub fn list_current_nodes(&self) -> Result<Vec<NodeRow>, StoreError> {
        self.list_live_nodes(false)
    }

    pub fn list_unfinalized_nodes(&self) -> Result<Vec<NodeRow>, StoreError> {
        self.list_live_nodes(true)
    }

    fn list_live_nodes(&self, include_stale: bool) -> Result<Vec<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM nodes WHERE state = 'active' OR (?1 AND state = 'stale') ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([include_stale], row_from_node)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_current_edges(&self) -> Result<Vec<EdgeRow>, StoreError> {
        self.list_live_edges(false)
    }

    pub fn list_unfinalized_edges(&self) -> Result<Vec<EdgeRow>, StoreError> {
        self.list_live_edges(true)
    }

    fn list_live_edges(&self, include_stale: bool) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE state = 'active' OR (?1 AND state = 'stale') ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([include_stale], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn mark_nodes_missing(
        &mut self,
        missing_ids: &[String],
        recorded_at_ns: i64,
        prefer_stale: bool,
        collector_run_id: Option<i64>,
    ) -> Result<usize, StoreError> {
        let mut count = 0usize;
        for id in missing_ids {
            let Some(mut row) = self.get_node(id)? else {
                continue;
            };
            if row.state != "active" && (row.state != "stale" || prefer_stale) {
                continue;
            }
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

    pub fn mark_edges_missing(
        &mut self,
        missing_ids: &[String],
        recorded_at_ns: i64,
        prefer_stale: bool,
        collector_run_id: Option<i64>,
    ) -> Result<usize, StoreError> {
        let mut count = 0usize;
        for id in missing_ids {
            let Some(mut row) = self.get_edge(id)? else {
                continue;
            };
            if row.state != "active" && (row.state != "stale" || prefer_stale) {
                continue;
            }
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

impl From<&NodeHistoryRow> for NodeRow {
    fn from(row: &NodeHistoryRow) -> Self {
        Self {
            id: row.node_id.clone(),
            kind: row.kind.clone(),
            label: row.label.clone(),
            state: row.state.clone(),
            first_seen_ns: row.first_seen_ns,
            last_seen_ns: row.last_seen_ns,
            valid_from_ns: row.valid_from_ns,
            valid_to_ns: row.valid_to_ns,
            metadata_json: row.metadata_json.clone(),
        }
    }
}

impl From<&EdgeHistoryRow> for EdgeRow {
    fn from(row: &EdgeHistoryRow) -> Self {
        Self {
            id: row.edge_id.clone(),
            from_node_id: row.from_node_id.clone(),
            to_node_id: row.to_node_id.clone(),
            kind: row.kind.clone(),
            class: row.class.clone(),
            state: row.state.clone(),
            evidence_score: row.evidence_score,
            evidence_label: row.evidence_label.clone(),
            evidence_count: row.evidence_count,
            first_seen_ns: row.first_seen_ns,
            last_seen_ns: row.last_seen_ns,
            metadata_json: row.metadata_json.clone(),
        }
    }
}
