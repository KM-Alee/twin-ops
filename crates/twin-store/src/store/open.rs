use std::path::Path;

use rusqlite::Connection;

use crate::error::StoreOpenError;
use crate::store::Store;

impl Store {
    pub fn open(path: &Path) -> Result<Self, StoreOpenError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|source| StoreOpenError::CreateDir {
                    path: parent.display().to_string(),
                    source,
                })?;
            }
        }
        let conn = Connection::open(path).map_err(|source| StoreOpenError::Open {
            path: path.display().to_string(),
            source,
        })?;
        let store = Self { conn };
        store.apply_pragmas()?;
        Ok(store)
    }

    pub fn open_in_memory() -> Result<Self, StoreOpenError> {
        let conn = Connection::open_in_memory().map_err(|source| StoreOpenError::Open {
            path: ":memory:".to_string(),
            source,
        })?;
        let store = Self { conn };
        store.apply_pragmas()?;
        Ok(store)
    }

    pub(crate) fn apply_pragmas(&self) -> Result<(), StoreOpenError> {
        self.conn
            .execute_batch(
                "PRAGMA journal_mode=WAL;
                 PRAGMA foreign_keys=ON;
                 PRAGMA busy_timeout=5000;
                 PRAGMA synchronous=NORMAL;",
            )
            .map_err(|source| StoreOpenError::Open {
                path: "pragma".to_string(),
                source,
            })?;
        Ok(())
    }
}
