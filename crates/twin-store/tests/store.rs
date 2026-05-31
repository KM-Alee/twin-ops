use std::path::Path;

use twin_store::{Store, StoreOpenError};

#[test]
fn open_in_memory_succeeds() {
    assert!(Store::open_in_memory().is_ok());
}

#[test]
fn initialize_creates_schema_migrations() {
    let store = Store::open_in_memory().expect("open");
    assert!(!store.is_initialized());
    store.initialize().expect("initialize");
    assert!(store.is_initialized());
    assert_eq!(store.schema_version().expect("version"), 1);
}

#[test]
fn initialize_is_idempotent() {
    let store = Store::open_in_memory().expect("open");
    store.initialize().expect("first");
    store.initialize().expect("second");
    assert_eq!(store.schema_version().expect("version"), 1);
}

#[test]
fn health_check_on_healthy_db() {
    let store = Store::open_in_memory().expect("open");
    store.initialize().expect("initialize");
    assert!(store.health_check().is_ok());
}

#[test]
fn open_nonexistent_parent_returns_create_dir_error() {
    let path = Path::new("/nonexistent/path/that/should/not/exist/twin.db");
    let result = Store::open(path);
    assert!(matches!(result, Err(StoreOpenError::CreateDir { .. })));
}
