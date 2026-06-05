use crate::error::StoreError;
use crate::migration::{self, LATEST_VERSION};
use crate::store::Store;

impl Store {
    pub fn initialize(&self) -> Result<(), StoreError> {
        self.apply_migrations_through(LATEST_VERSION)
    }

    /// Applies pending migrations through `target` (inclusive).
    #[doc(hidden)]
    pub fn apply_migrations_through(&self, target: i64) -> Result<(), StoreError> {
        let current = self.schema_version().map_err(|e| match e {
            StoreError::Query { source } => StoreError::Migration { version: 0, source },
            other => other,
        })?;

        for version in (current + 1)..=target {
            self.run_migration(version)?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64, StoreError> {
        if !self.is_initialized()? {
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

    pub fn is_initialized(&self) -> Result<bool, StoreError> {
        match self.conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations'",
            [],
            |_| Ok(()),
        ) {
            Ok(()) => Ok(true),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(false),
            Err(source) => Err(StoreError::Query { source }),
        }
    }

    fn run_migration(&self, version: i64) -> Result<(), StoreError> {
        let sql = match version {
            1 => migration::MIGRATION_001,
            2 => migration::MIGRATION_002,
            3 => migration::MIGRATION_003,
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
