use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::edge::row_from_edge;
use crate::repo::node::row_from_node;
use crate::repo::observation::collect_rows;
use crate::repo::{EdgeRow, NodeRow};
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotRow {
    pub name: String,
    pub created_at_ns: i64,
    pub node_count: i64,
    pub edge_count: i64,
}

#[derive(Debug, thiserror::Error)]
pub enum SnapshotError {
    #[error("snapshot `{name}` already exists")]
    DuplicateName { name: String },
    #[error("no active graph rows to snapshot")]
    EmptyGraph,
    #[error("snapshot `{name}` not found")]
    NotFound { name: String },
}

impl Store {
    pub fn create_snapshot(
        &mut self,
        name: &str,
        created_at_ns: i64,
    ) -> Result<SnapshotRow, StoreError> {
        if self.snapshot_exists(name)? {
            return Err(StoreError::Snapshot(SnapshotError::DuplicateName {
                name: name.to_string(),
            }));
        }
        self.with_transaction(|store| {
            let nodes = store.list_current_nodes()?;
            let edges = store.list_current_edges()?;
            if nodes.is_empty() && edges.is_empty() {
                return Err(StoreError::Snapshot(SnapshotError::EmptyGraph));
            }
            store
                .conn
                .execute(
                    "INSERT INTO snapshots (name, created_at_ns, node_count, edge_count)
                 VALUES (?1, ?2, ?3, ?4)",
                    params![name, created_at_ns, nodes.len() as i64, edges.len() as i64],
                )
                .map_err(|source| StoreError::Upsert { source })?;
            for node in &nodes {
                store
                    .conn
                    .execute(
                        "INSERT INTO snapshot_nodes (
                        snapshot_name, id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                        params![
                            name,
                            node.id,
                            node.kind,
                            node.label,
                            node.state,
                            node.first_seen_ns,
                            node.last_seen_ns,
                            node.valid_from_ns,
                            node.valid_to_ns,
                            node.metadata_json,
                        ],
                    )
                    .map_err(|source| StoreError::Upsert { source })?;
            }
            for edge in &edges {
                store
                    .conn
                    .execute(
                        "INSERT INTO snapshot_edges (
                        snapshot_name, id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                    ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                        params![
                            name,
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
                        ],
                    )
                    .map_err(|source| StoreError::Upsert { source })?;
            }
            store.conn.execute(
                "INSERT INTO snapshot_edge_observations (snapshot_name, edge_id, observation_id, role)
                 SELECT ?1, eo.edge_id, eo.observation_id, eo.role FROM edge_observations eo
                 JOIN snapshot_edges se ON se.snapshot_name = ?1 AND se.id = eo.edge_id
                 JOIN observations o ON o.id = eo.observation_id
                 WHERE o.timestamp_ns <= ?2",
                params![name, created_at_ns],
            ).map_err(|source| StoreError::Insert { source })?;
            Ok(SnapshotRow {
                name: name.to_string(),
                created_at_ns,
                node_count: nodes.len() as i64,
                edge_count: edges.len() as i64,
            })
        })
    }

    pub fn load_snapshot_observations_for_edge(
        &self,
        name: &str,
        edge_id: &str,
    ) -> Result<Vec<(String, String)>, StoreError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT observation_id, role FROM snapshot_edge_observations
             WHERE snapshot_name = ?1 AND edge_id = ?2 ORDER BY observation_id, role",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = statement
            .query_map(params![name, edge_id], |row| Ok((row.get(0)?, row.get(1)?)))
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_snapshots(&self) -> Result<Vec<SnapshotRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT name, created_at_ns, node_count, edge_count
                 FROM snapshots ORDER BY created_at_ns DESC",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], row_from_snapshot)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn snapshot_exists(&self, name: &str) -> Result<bool, StoreError> {
        let count: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM snapshots WHERE name = ?1",
                params![name],
                |row| row.get(0),
            )
            .map_err(|source| StoreError::Query { source })?;
        Ok(count > 0)
    }

    pub fn load_snapshot_nodes(&self, name: &str) -> Result<Vec<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM snapshot_nodes WHERE snapshot_name = ?1 ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map(params![name], row_from_node)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn load_snapshot_edges(&self, name: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM snapshot_edges WHERE snapshot_name = ?1 ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map(params![name], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }
}

fn row_from_snapshot(row: &Row<'_>) -> Result<SnapshotRow, rusqlite::Error> {
    Ok(SnapshotRow {
        name: row.get(0)?,
        created_at_ns: row.get(1)?,
        node_count: row.get(2)?,
        edge_count: row.get(3)?,
    })
}
