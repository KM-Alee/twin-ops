mod support;

use std::path::Path;

use twin_store::{Store, StoreOpenError, LATEST_VERSION};

#[test]
fn open_in_memory_succeeds() {
    assert!(Store::open_in_memory().is_ok());
}

#[test]
fn initialize_creates_schema_migrations() {
    let store = Store::open_in_memory().expect("open");
    assert!(!store.is_initialized().expect("check"));
    store.initialize().expect("initialize");
    assert!(store.is_initialized().expect("check"));
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
}

#[test]
fn health_check_on_healthy_db() {
    let store = support::blank_store();
    assert!(store.health_check().is_ok());
}

#[test]
fn open_nonexistent_parent_returns_create_dir_error() {
    let path = Path::new("/nonexistent/path/that/should/not/exist/twin.db");
    let result = Store::open(path);
    assert!(matches!(result, Err(StoreOpenError::CreateDir { .. })));
}
