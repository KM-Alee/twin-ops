mod support;

use twin_core::ChangeKind;
use twin_store::{Store, LATEST_VERSION};

#[test]
fn migration_004_creates_history_and_snapshot_tables() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("twin.db");
    let store = Store::open(&path).expect("open");
    store.initialize().expect("initialize");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    for table in [
        "node_history",
        "edge_history",
        "snapshots",
        "snapshot_nodes",
        "snapshot_edges",
    ] {
        assert!(support::table_exists(&path, table), "missing table {table}");
    }
}

#[test]
fn upgrades_existing_v3_database() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("twin.db");
    let store = support::open_v3_only(&path);
    assert!(!support::table_exists(&path, "node_history"));
    store.initialize().expect("migrate to v4");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    assert!(support::table_exists(&path, "snapshots"));
}

#[test]
fn history_records_node_and_edge_transitions() {
    let mut store = support::blank_store();
    let node = support::node_row("process:pid:42", "process", "nginx", support::TS);
    store
        .insert_node_history(&node, ChangeKind::New, support::TS, None)
        .expect("node history");
    let edge = support::edge_row(
        "process:pid:1->parent_of->process:pid:42",
        "process:pid:1",
        "process:pid:42",
        "parent_of",
        support::TS,
    );
    store
        .insert_edge_history(&edge, ChangeKind::New, support::TS, None)
        .expect("edge history");

    let since = support::TS - 1;
    assert_eq!(
        store.list_node_history_since(since).expect("nodes").len(),
        1
    );
    assert_eq!(
        store.list_edge_history_since(since).expect("edges").len(),
        1
    );
}

#[test]
fn snapshot_create_copies_current_graph_rows() {
    let mut store = support::blank_store();
    support::seed_ab_nodes(&mut store);
    store
        .upsert_edge(&support::edge_row(
            "a->parent_of->b",
            "a",
            "b",
            "parent_of",
            support::TS,
        ))
        .expect("edge");
    let snap = store
        .create_snapshot("before-change", support::TS)
        .expect("snapshot");
    assert_eq!(snap.node_count, 2);
    assert_eq!(snap.edge_count, 1);
    let nodes = store
        .load_snapshot_nodes("before-change")
        .expect("snapshot nodes");
    assert_eq!(nodes.len(), 2);
    assert_eq!(nodes[0].label, "a");
}

#[test]
fn snapshot_duplicate_name_is_rejected() {
    let mut store = support::blank_store();
    support::seed_ab_nodes(&mut store);
    store
        .create_snapshot("dup", support::TS)
        .expect("first snapshot");
    let err = store
        .create_snapshot("dup", support::TS + 1)
        .expect_err("duplicate");
    assert!(err.to_string().contains("already exists"));
}
