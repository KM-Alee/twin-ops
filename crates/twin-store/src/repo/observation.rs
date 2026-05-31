use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservationRow {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub subject_node_id: Option<String>,
    pub object_node_id: Option<String>,
    pub timestamp_ns: i64,
    pub confidence_hint: String,
    pub redaction_state: String,
    pub metadata_json: String,
    pub collector_run_id: Option<i64>,
}

impl Store {
    pub fn insert_observation(&mut self, obs: &ObservationRow) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO observations (
                    id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                    confidence_hint, redaction_state, metadata_json, collector_run_id
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![
                    obs.id,
                    obs.source,
                    obs.kind,
                    obs.subject_node_id,
                    obs.object_node_id,
                    obs.timestamp_ns,
                    obs.confidence_hint,
                    obs.redaction_state,
                    obs.metadata_json,
                    obs.collector_run_id,
                ],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(())
    }

    pub fn insert_observations(&mut self, obs: &[ObservationRow]) -> Result<(), StoreError> {
        self.with_transaction(|store| {
            for row in obs {
                store.insert_observation(row)?;
            }
            Ok(())
        })
    }

    pub fn get_observation(&self, id: &str) -> Result<Option<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE id = ?1",
            )
            .map_err(|source| StoreError::Query { source })?;

        let mut rows = stmt
            .query(params![id])
            .map_err(|source| StoreError::Query { source })?;

        match rows.next().map_err(|source| StoreError::Query { source })? {
            Some(row) => Ok(Some(
                row_from_observation(row).map_err(|source| StoreError::Query { source })?,
            )),
            None => Ok(None),
        }
    }

    pub fn list_observations_by_source(
        &self,
        source: &str,
    ) -> Result<Vec<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE source = ?1 ORDER BY timestamp_ns",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![source], row_from_observation)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn list_observations_by_subject(
        &self,
        node_id: &str,
    ) -> Result<Vec<ObservationRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, source, kind, subject_node_id, object_node_id, timestamp_ns,
                        confidence_hint, redaction_state, metadata_json, collector_run_id
                 FROM observations WHERE subject_node_id = ?1 ORDER BY timestamp_ns",
            )
            .map_err(|source| StoreError::Query { source })?;

        let rows = stmt
            .query_map(params![node_id], row_from_observation)
            .map_err(|source| StoreError::Query { source })?;

        collect_rows(rows)
    }

    pub fn count_observations(&self) -> Result<i64, StoreError> {
        self.conn
            .query_row("SELECT COUNT(*) FROM observations", [], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })
    }
}

fn row_from_observation(row: &Row<'_>) -> Result<ObservationRow, rusqlite::Error> {
    Ok(ObservationRow {
        id: row.get(0)?,
        source: row.get(1)?,
        kind: row.get(2)?,
        subject_node_id: row.get(3)?,
        object_node_id: row.get(4)?,
        timestamp_ns: row.get(5)?,
        confidence_hint: row.get(6)?,
        redaction_state: row.get(7)?,
        metadata_json: row.get(8)?,
        collector_run_id: row.get(9)?,
    })
}

pub(crate) fn collect_rows<T, F>(rows: rusqlite::MappedRows<'_, F>) -> Result<Vec<T>, StoreError>
where
    F: FnMut(&Row<'_>) -> Result<T, rusqlite::Error>,
{
    rows.map(|row| row.map_err(|source| StoreError::Query { source }))
        .collect()
}
