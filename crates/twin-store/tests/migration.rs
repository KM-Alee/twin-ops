mod support;

use twin_store::{Store, LATEST_VERSION};

#[test]
fn fresh_database_has_domain_tables_at_latest_version() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("twin.db");
    let store = Store::open(&path).expect("open");
    store.initialize().expect("initialize");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    for table in [
        "observations",
        "nodes",
        "edges",
        "edge_observations",
        "collector_runs",
        "node_history",
        "edge_history",
        "snapshots",
        "snapshot_nodes",
        "snapshot_edges",
    ] {
        assert!(support::table_exists(&path, table), "missing table {table}");
    }
    assert_eq!(store.count_observations().expect("count"), 0);
    assert_eq!(store.count_nodes().expect("count"), 0);
    assert_eq!(store.count_edges().expect("count"), 0);
}

#[test]
fn upgrades_existing_v1_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("twin.db");
    let store = support::open_v1_only(&path);
    assert!(!support::table_exists(&path, "nodes"));
    store.initialize().expect("migrate");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    assert!(support::table_exists(&path, "nodes"));
}

#[test]
fn upgrades_v2_adds_collector_run_metadata_json() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("twin.db");
    let store = support::open_v2_only(&path);
    assert!(
        !support::column_exists(&path, "collector_runs", "metadata_json"),
        "v2 schema should not have metadata_json yet"
    );
    store.initialize().expect("migrate to v3");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    assert!(support::column_exists(
        &path,
        "collector_runs",
        "metadata_json"
    ));
    let mut store = Store::open(&path).expect("reopen");
    store
        .insert_collector_run(&support::collector_run_row("proc_process", 1, 2))
        .expect("insert");
    let run = store
        .latest_collector_run("proc_process")
        .expect("latest")
        .expect("run");
    assert_eq!(run.metadata_json, "{}");
}

#[test]
fn initialize_twice_is_idempotent() {
    let store = Store::open_in_memory().expect("open");
    store.initialize().expect("first");
    store.initialize().expect("second");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
}
