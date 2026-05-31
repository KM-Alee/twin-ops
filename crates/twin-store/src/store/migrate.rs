use crate::error::StoreError;
use crate::migration::{self, LATEST_VERSION};
use crate::store::Store;

impl Store {
    pub fn initialize(&self) -> Result<(), StoreError> {
        let current = self.schema_version().map_err(|e| match e {
            StoreError::Query { source } => StoreError::Migration { version: 0, source },
            other => other,
        })?;

        for version in (current + 1)..=LATEST_VERSION {
            self.run_migration(version)?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        if !self.is_initialized() {
            return Ok(0);
        }
        let version: i64 = self
            .conn
            .query_row(
                "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .map_err(|source| StoreError::Query { source })?;
        Ok(version)
    }

    pub fn is_initialized(&self) -> bool {
        self.conn
            .prepare(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            )
            .and_then(|mut stmt| stmt.query_row([], |row| row.get::<_, i32>(0)).map(|_| true))
            .unwrap_or(false)
    }

    fn run_migration(&self, version: i64) -> Result<(), StoreError> {
        let sql = match version {
            1 => migration::MIGRATION_001,
            _ => {
                return Err(StoreError::Migration {
                    version,
                    source: rusqlite::Error::InvalidParameterName(format!(
                        "unknown migration version {version}"
                    )),
                });
            }
        };

        self.conn
            .execute_batch(sql)
            .map_err(|source| StoreError::Migration { version, source })?;

        let applied_at_ns = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as i64)
            .unwrap_or(0);

        self.conn
            .execute(
                "INSERT INTO schema_migrations (version, applied_at_ns) VALUES (?1, ?2)",
                rusqlite::params![version, applied_at_ns],
            )
            .map_err(|source| StoreError::Migration { version, source })?;

        Ok(())
    }
}
