use crate::error::StoreError;
use crate::store::Store;

impl Store {
    pub fn health_check(&self) -> Result<(), StoreError> {
        self.conn
            .query_row("SELECT 1", [], |_| Ok(()))
            .map_err(|source| StoreError::HealthCheck { source })?;
        Ok(())
    }

    pub fn journal_mode_wal(&self) -> Result<bool, StoreError> {
        let mode: String = self
            .conn
            .query_row("PRAGMA journal_mode", [], |row| row.get(0))
            .map_err(|source| StoreError::Query { source })?;
        Ok(mode.eq_ignore_ascii_case("wal"))
    }
}
