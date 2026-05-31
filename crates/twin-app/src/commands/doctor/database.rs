use std::path::Path;

use twin_store::Store;

use crate::model::DoctorDatabase;

pub fn check(db_path: &Path) -> DoctorDatabase {
    let mut result = DoctorDatabase {
        initialized: false,
        schema_version: None,
        db_path: Some(db_path.to_path_buf()),
        wal_mode: None,
    };

    if !db_path.exists() {
        return result;
    }

    let store = match Store::open(db_path) {
        Ok(store) => store,
        Err(_) => return result,
    };

    let initialized = store.is_initialized().unwrap_or(false);
    if !initialized || store.health_check().is_err() {
        return result;
    }

    result.initialized = true;
    result.schema_version = store.schema_version().ok();
    result.wal_mode = store.journal_mode_wal().ok();
    result
}
