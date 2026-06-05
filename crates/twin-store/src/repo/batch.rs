use std::collections::HashMap;

use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::{NodeRow, ObservationRow};
use crate::store::Store;

const BATCH_CHUNK: usize = 500;

impl Store {
    pub fn count_nodes_by_kind(&self, kind: &str) -> Result<i64, StoreError> {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM nodes WHERE kind = ?1",
                params![kind],
                |row| row.get(0),
            )
            .map_err(|source| StoreError::Query { source })
    }

    pub fn get_nodes_by_ids(&self, ids: &[&str]) -> Result<HashMap<String, NodeRow>, StoreError> {
        query_by_ids(
            &self.conn,
            "nodes",
            node_select_sql(),
            ids,
            super::node::row_from_node,
        )
    }

    pub fn get_observations_by_ids(
        &self,
        ids: &[&str],
    ) -> Result<HashMap<String, ObservationRow>, StoreError> {
        query_by_ids(
            &self.conn,
            "observations",
            observation_select_sql(),
            ids,
            super::observation::row_from_observation,
        )
    }
}

fn node_select_sql() -> &'static str {
    "SELECT id, kind, label, state, first_seen_ns, last_seen_ns, valid_from_ns, valid_to_ns, metadata_json FROM nodes WHERE id IN "
}

fn observation_select_sql() -> &'static str {
    "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns, confidence_hint, redaction_state, metadata_json, collector_run_id FROM observations WHERE id IN "
}

fn query_by_ids<F, R>(
    conn: &rusqlite::Connection,
    _table: &str,
    select_prefix: &str,
    ids: &[&str],
    map_row: F,
) -> Result<HashMap<String, R>, StoreError>
where
    F: Fn(&Row<'_>) -> Result<R, rusqlite::Error>,
    R: HasRowId,
{
    let mut out = HashMap::new();
    if ids.is_empty() {
        return Ok(out);
    }
    for chunk in ids.chunks(BATCH_CHUNK) {
        let placeholders: Vec<String> = (0..chunk.len()).map(|i| format!("?{}", i + 1)).collect();
        let sql = format!("{}({})", select_prefix, placeholders.join(", "));
        let mut stmt = conn
            .prepare(&sql)
            .map_err(|source| StoreError::Query { source })?;
        let params: Vec<&dyn rusqlite::ToSql> =
            chunk.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let mut rows = stmt
            .query(params.as_slice())
            .map_err(|source| StoreError::Query { source })?;
        while let Some(row) = rows.next().map_err(|source| StoreError::Query { source })? {
            let mapped = map_row(row).map_err(|source| StoreError::Query { source })?;
            out.insert(mapped.row_id(), mapped);
        }
    }
    Ok(out)
}

trait HasRowId {
    fn row_id(&self) -> String;
}

impl HasRowId for NodeRow {
    fn row_id(&self) -> String {
        self.id.clone()
    }
}

impl HasRowId for ObservationRow {
    fn row_id(&self) -> String {
        self.id.clone()
    }
}
