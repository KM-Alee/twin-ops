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

#[test]
fn previous_history_is_entity_scoped_and_breaks_timestamp_ties_by_id() {
    let mut store = support::blank_store();
    let mut node = support::node_row("a", "process", "before", 100);
    store
        .insert_node_history(&node, ChangeKind::New, 100, None)
        .unwrap();
    node.label = "after".into();
    store
        .insert_node_history(&node, ChangeKind::Changed, 100, None)
        .unwrap();
    store
        .insert_node_history(
            &support::node_row("b", "process", "other", 100),
            ChangeKind::New,
            100,
            None,
        )
        .unwrap();
    let rows = store.list_node_history_since(0).unwrap();
    assert!(store.node_history_before(&rows[0]).unwrap().is_none());
    assert_eq!(
        store.node_history_before(&rows[1]).unwrap().unwrap().label,
        "before"
    );
    assert!(store.node_history_before(&rows[2]).unwrap().is_none());
}

#[test]
fn graph_at_reconstructs_latest_versions_without_future_rows() {
    use twin_core::TimestampNs;
    let mut store = support::blank_store();
    let mut node = support::node_row("a", "process", "first", 100);
    store
        .insert_node_history(&node, ChangeKind::New, 100, None)
        .unwrap();
    node.label = "second".into();
    store
        .insert_node_history(&node, ChangeKind::Changed, 200, None)
        .unwrap();
    node.label = "same-time-later-id".into();
    store
        .insert_node_history(&node, ChangeKind::Changed, 200, None)
        .unwrap();
    node.state = "gone".into();
    store
        .insert_node_history(&node, ChangeKind::Gone, 300, None)
        .unwrap();
    assert!(store.nodes_at(TimestampNs::new(99)).unwrap().is_empty());
    assert_eq!(
        store.nodes_at(TimestampNs::new(100)).unwrap()[0].label,
        "first"
    );
    assert_eq!(
        store.nodes_at(TimestampNs::new(200)).unwrap()[0].label,
        "same-time-later-id"
    );
    assert!(store.nodes_at(TimestampNs::new(300)).unwrap().is_empty());
    let edge = support::edge_row("a->parent_of->b", "a", "b", "parent_of", 100);
    store
        .insert_edge_history(&edge, ChangeKind::New, 100, None)
        .unwrap();
    assert!(store.edges_at(TimestampNs::new(99)).unwrap().is_empty());
    assert_eq!(store.edges_at(TimestampNs::new(100)).unwrap().len(), 1);
}

#[test]
fn stale_rows_can_be_finalized_when_a_later_complete_scan_confirms_absence() {
    let mut store = support::blank_store();
    for (id, label) in [("process:pid:1", "parent"), ("process:pid:42", "child")] {
        store
            .upsert_node(&support::node_row(id, "process", label, 10))
            .unwrap();
    }
    let id = "process:pid:1|parent_of|process:pid:42";
    store
        .upsert_edge(&support::edge_row(
            id,
            "process:pid:1",
            "process:pid:42",
            "parent_of",
            10,
        ))
        .unwrap();
    let nodes = vec!["process:pid:42".to_string()];
    let edges = vec![id.to_string()];
    assert_eq!(store.mark_nodes_missing(&nodes, 20, true, None).unwrap(), 1);
    assert_eq!(store.mark_edges_missing(&edges, 20, true, None).unwrap(), 1);
    assert_eq!(store.list_current_nodes().unwrap().len(), 1);
    assert_eq!(store.list_unfinalized_nodes().unwrap().len(), 2);
    assert_eq!(store.list_unfinalized_edges().unwrap().len(), 1);
    assert_eq!(store.mark_nodes_missing(&nodes, 30, true, None).unwrap(), 0);
    assert_eq!(store.mark_edges_missing(&edges, 30, true, None).unwrap(), 0);
    assert_eq!(
        store.mark_nodes_missing(&nodes, 40, false, None).unwrap(),
        1
    );
    assert_eq!(
        store.mark_edges_missing(&edges, 40, false, None).unwrap(),
        1
    );
    assert_eq!(
        store
            .get_node("process:pid:42")
            .unwrap()
            .unwrap()
            .valid_to_ns,
        Some(40)
    );
    assert_eq!(store.get_edge(id).unwrap().unwrap().state, "gone");
    assert_eq!(store.list_unfinalized_nodes().unwrap().len(), 1);
    assert!(store.list_unfinalized_edges().unwrap().is_empty());
}
