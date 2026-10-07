use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::observation::collect_rows;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRow {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub state: String,
    pub first_seen_ns: i64,
    pub last_seen_ns: i64,
    pub valid_from_ns: i64,
    pub valid_to_ns: Option<i64>,
    pub metadata_json: String,
}

impl Store {
    pub fn upsert_node(&mut self, node: &NodeRow) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO nodes (
                    id, kind, label, state, first_seen_ns, last_seen_ns,
                    valid_from_ns, valid_to_ns, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                ON CONFLICT(id) DO UPDATE SET
                    kind = excluded.kind,
                    label = excluded.label,
                    state = excluded.state,
                    last_seen_ns = excluded.last_seen_ns,
                    valid_to_ns = excluded.valid_to_ns,
                    metadata_json = excluded.metadata_json",
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
                ],
            )
            .map_err(|source| StoreError::Upsert { source })?;
        Ok(())
    }

    pub fn upsert_nodes(&mut self, nodes: &[NodeRow]) -> Result<(), StoreError> {
        self.with_transaction(|store| {
            for node in nodes {
                store.upsert_node(node)?;
            }
            Ok(())
        })
    }

    pub fn get_node(&self, id: &str) -> Result<Option<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM nodes WHERE id = ?1",
            )
            .map_err(|source| StoreError::Query { source })?;

        let mut rows = stmt
            .query(params![id])
            .map_err(|source| StoreError::Query { source })?;

        match rows.next().map_err(|source| StoreError::Query { source })? {
            Some(row) => Ok(Some(
                row_from_node(row).map_err(|source| StoreError::Query { source })?,
            )),
            None => Ok(None),
        }
    }

    pub fn list_nodes(&self) -> Result<Vec<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM nodes ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], row_from_node)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }

    pub fn list_nodes_by_kind(&self, kind: &str) -> Result<Vec<NodeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, kind, label, state, first_seen_ns, last_seen_ns,
                        valid_from_ns, valid_to_ns, metadata_json
                 FROM nodes WHERE kind = ?1 AND state = 'active' ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![kind], row_from_node)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn count_nodes(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })
    }
}

pub(crate) fn row_from_node(row: &Row<'_>) -> Result<NodeRow, rusqlite::Error> {
    Ok(NodeRow {
        id: row.get(0)?,
        kind: row.get(1)?,
        label: row.get(2)?,
        state: row.get(3)?,
        first_seen_ns: row.get(4)?,
        last_seen_ns: row.get(5)?,
        valid_from_ns: row.get(6)?,
        valid_to_ns: row.get(7)?,
        metadata_json: row.get(8)?,
    })
}
