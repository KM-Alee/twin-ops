use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::observation::collect_rows;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectorRunRow {
    pub id: Option<i64>,
    pub collector: String,
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub status: String,
    pub observation_count: i64,
    pub warning_count: i64,
    pub error_message: Option<String>,
    pub metadata_json: String,
}

impl Store {
    pub fn insert_collector_run(&mut self, run: &CollectorRunRow) -> Result<i64, StoreError> {
        self.conn
            .execute(
                "INSERT INTO collector_runs (
                    collector, started_at_ns, ended_at_ns, status,
                    observation_count, warning_count, error_message, metadata_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    run.collector,
                    run.started_at_ns,
                    run.ended_at_ns,
                    run.status,
                    run.observation_count,
                    run.warning_count,
                    run.error_message,
                    run.metadata_json,
                ],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(self.conn.last_insert_rowid())
    }

    pub fn list_collector_runs(
        &self,
        collector: Option<&str>,
    ) -> Result<Vec<CollectorRunRow>, StoreError> {
        match collector {
            Some(name) => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, collector, started_at_ns, ended_at_ns, status,
                                observation_count, warning_count, error_message, metadata_json
                         FROM collector_runs WHERE collector = ?1
                         ORDER BY started_at_ns",
                    )
                    .map_err(|source| StoreError::Query { source })?;
                let rows = stmt
                    .query_map(params![name], row_from_collector_run)
                    .map_err(|source| StoreError::Query { source })?;
                collect_rows(rows)
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare(
                        "SELECT id, collector, started_at_ns, ended_at_ns, status,
                                observation_count, warning_count, error_message, metadata_json
                         FROM collector_runs ORDER BY started_at_ns",
                    )
                    .map_err(|source| StoreError::Query { source })?;
                let rows = stmt
                    .query_map([], row_from_collector_run)
                    .map_err(|source| StoreError::Query { source })?;
                collect_rows(rows)
            }
        }
    }

    pub fn update_collector_run_metadata(
        &mut self,
        run_id: i64,
        metadata_json: &str,
    ) -> Result<(), StoreError> {
        self.conn
            .execute(
                "UPDATE collector_runs SET metadata_json = ?1 WHERE id = ?2",
                params![metadata_json, run_id],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(())
    }

    pub fn latest_collector_run(
        &self,
        collector: &str,
    ) -> Result<Option<CollectorRunRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, collector, started_at_ns, ended_at_ns, status,
                        observation_count, warning_count, error_message, metadata_json
                 FROM collector_runs
                 WHERE collector = ?1
                 ORDER BY started_at_ns DESC
                 LIMIT 1",
            )
            .map_err(|source| StoreError::Query { source })?;

        let mut rows = stmt
            .query(params![collector])
            .map_err(|source| StoreError::Query { source })?;

        match rows.next().map_err(|source| StoreError::Query { source })? {
            Some(row) => Ok(Some(
                row_from_collector_run(row).map_err(|source| StoreError::Query { source })?,
            )),
            None => Ok(None),
        }
    }
}

fn row_from_collector_run(row: &Row<'_>) -> Result<CollectorRunRow, rusqlite::Error> {
    Ok(CollectorRunRow {
        id: Some(row.get(0)?),
        collector: row.get(1)?,
        started_at_ns: row.get(2)?,
        ended_at_ns: row.get(3)?,
        status: row.get(4)?,
        observation_count: row.get(5)?,
        warning_count: row.get(6)?,
        error_message: row.get(7)?,
        metadata_json: row.get(8)?,
    })
}
