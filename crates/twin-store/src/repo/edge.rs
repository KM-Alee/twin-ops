use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::observation::collect_rows;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeRow {
    pub id: String,
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
}

impl Store {
    pub fn upsert_edge(&mut self, edge: &EdgeRow) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO edges (
                    id, from_node_id, to_node_id, kind, class, state,
                    evidence_score, evidence_label, evidence_count,
                    first_seen_ns, last_seen_ns, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                ON CONFLICT(id) DO UPDATE SET
                    from_node_id = excluded.from_node_id,
                    to_node_id = excluded.to_node_id,
                    kind = excluded.kind,
                    class = excluded.class,
                    state = excluded.state,
                    evidence_score = excluded.evidence_score,
                    evidence_label = excluded.evidence_label,
                    evidence_count = excluded.evidence_count,
                    last_seen_ns = excluded.last_seen_ns,
                    metadata_json = excluded.metadata_json",
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
                ],
            )
            .map_err(|source| StoreError::Upsert { source })?;
        Ok(())
    }

    pub fn upsert_edges(&mut self, edges: &[EdgeRow]) -> Result<(), StoreError> {
        self.with_transaction(|store| {
            for edge in edges {
                store.upsert_edge(edge)?;
            }
            Ok(())
        })
    }

    pub fn get_edge(&self, id: &str) -> Result<Option<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE id = ?1",
            )
            .map_err(|source| StoreError::Query { source })?;

        let mut rows = stmt
            .query(params![id])
            .map_err(|source| StoreError::Query { source })?;

        match rows.next().map_err(|source| StoreError::Query { source })? {
            Some(row) => Ok(Some(
                row_from_edge(row).map_err(|source| StoreError::Query { source })?,
            )),
            None => Ok(None),
        }
    }

    pub fn list_edges_from(&self, node_id: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE from_node_id = ?1 ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![node_id], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn list_edges_to(&self, node_id: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE to_node_id = ?1 ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![node_id], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn list_edges_by_kind(&self, kind: &str) -> Result<Vec<EdgeRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, from_node_id, to_node_id, kind, class, state,
                        evidence_score, evidence_label, evidence_count,
                        first_seen_ns, last_seen_ns, metadata_json
                 FROM edges WHERE kind = ?1 ORDER BY id",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![kind], row_from_edge)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn count_edges(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })
    }
}

fn row_from_edge(row: &Row<'_>) -> Result<EdgeRow, rusqlite::Error> {
    Ok(EdgeRow {
        id: row.get(0)?,
        from_node_id: row.get(1)?,
        to_node_id: row.get(2)?,
        kind: row.get(3)?,
        class: row.get(4)?,
        state: row.get(5)?,
        evidence_score: row.get(6)?,
        evidence_label: row.get(7)?,
        evidence_count: row.get(8)?,
        first_seen_ns: row.get(9)?,
        last_seen_ns: row.get(10)?,
        metadata_json: row.get(11)?,
    })
}
