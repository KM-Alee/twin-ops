use rusqlite::{params, Row};

use crate::error::StoreError;
use crate::repo::observation::collect_rows;
use crate::store::Store;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestRunRow {
    pub id: String,
    pub name: String,
    pub file_path: String,
    pub started_at_ns: i64,
    pub passed: i64,
    pub warned: i64,
    pub failed: i64,
    pub report_json: String,
}

impl Store {
    pub fn insert_test_run(&mut self, row: &TestRunRow) -> Result<(), StoreError> {
        self.conn
            .execute(
                "INSERT INTO test_runs (
                    id, name, file_path, started_at_ns, passed, warned, failed, report_json
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    row.id,
                    row.name,
                    row.file_path,
                    row.started_at_ns,
                    row.passed,
                    row.warned,
                    row.failed,
                    row.report_json,
                ],
            )
            .map_err(|source| StoreError::Insert { source })?;
        Ok(())
    }

    pub fn list_test_runs(&self) -> Result<Vec<TestRunRow>, StoreError> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, name, file_path, started_at_ns, passed, warned, failed, report_json
                 FROM test_runs ORDER BY started_at_ns, id",
            )
            .map_err(|source| StoreError::Query { source })?;
        let rows = stmt
            .query_map([], row_from_test_run)
            .map_err(|source| StoreError::Query { source })?;
        collect_rows(rows)
    }
}

fn row_from_test_run(row: &Row<'_>) -> Result<TestRunRow, rusqlite::Error> {
    Ok(TestRunRow {
        id: row.get(0)?,
        name: row.get(1)?,
        file_path: row.get(2)?,
        started_at_ns: row.get(3)?,
        passed: row.get(4)?,
        warned: row.get(5)?,
        failed: row.get(6)?,
        report_json: row.get(7)?,
    })
}
