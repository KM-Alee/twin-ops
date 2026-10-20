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

#[test]
fn v6_removes_legacy_arguments_from_current_history_and_snapshots() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("legacy.db");
    let store = Store::open(&path).expect("store");
    store.apply_migrations_through(5).expect("v5");
    let conn = rusqlite::Connection::open(&path).expect("fixture connection");
    let metadata = r#"{"argv_json":"[\"curl\",\"SYNTHETIC_SENTINEL\"]","argv":["curl","SYNTHETIC_SENTINEL"],"pid":42,"nested":{"argv":["SYNTHETIC_SENTINEL"],"safe":"retained"},"array":[{"argv_json":"SYNTHETIC_SENTINEL","safe":7}]}"#;
    conn.execute(
        "INSERT INTO nodes VALUES ('process:pid:42','process','api','active',1,1,1,NULL,?1)",
        [metadata],
    )
    .expect("node");
    conn.execute("INSERT INTO edges VALUES ('edge','process:pid:42','process:pid:42','parent_of','observed','active',75,'strong',1,1,1,?1)", [metadata]).expect("edge");
    conn.execute("INSERT INTO observations VALUES ('obs','proc','process_seen',NULL,NULL,1,'high','none',?1,NULL)", [metadata]).expect("observation");
    conn.execute("INSERT INTO observations VALUES ('malformed','proc','process_seen',NULL,NULL,1,'high','none','{SYNTHETIC_SENTINEL',NULL)", []).expect("malformed");
    conn.execute_batch("INSERT INTO node_history (node_id,kind,label,state,first_seen_ns,last_seen_ns,valid_from_ns,valid_to_ns,metadata_json,change_kind,recorded_at_ns) SELECT id,kind,label,state,first_seen_ns,last_seen_ns,valid_from_ns,valid_to_ns,metadata_json,'new',1 FROM nodes;
INSERT INTO edge_history (edge_id,from_node_id,to_node_id,kind,class,state,evidence_score,evidence_label,evidence_count,first_seen_ns,last_seen_ns,metadata_json,change_kind,recorded_at_ns) SELECT id,from_node_id,to_node_id,kind,class,state,evidence_score,evidence_label,evidence_count,first_seen_ns,last_seen_ns,metadata_json,'new',1 FROM edges;
INSERT INTO snapshots VALUES ('baseline',1,1,1);
INSERT INTO snapshot_nodes SELECT 'baseline', * FROM nodes;
INSERT INTO snapshot_edges SELECT 'baseline', * FROM edges;").expect("history and snapshot");
    conn.execute("INSERT INTO collector_runs (collector,started_at_ns,ended_at_ns,status,error_message,metadata_json) VALUES ('docker',1,1,'unavailable','SYNTHETIC_SENTINEL',?1)", [r#"{"error":"SYNTHETIC_SENTINEL"}"#]).expect("docker error");
    conn.execute("INSERT INTO test_runs VALUES ('legacy','contract','twin.yaml',1,1,0,0,?1)", [r#"{"passed":1,"checks":[{"details":{"argv":["SYNTHETIC_SENTINEL"],"safe":"retained"}}]}"#]).expect("legacy report");
    store.apply_migrations_through(6).expect("upgrade");
    assert_eq!(store.schema_version().expect("version"), 6);
    for table in [
        "observations",
        "nodes",
        "edges",
        "node_history",
        "edge_history",
        "snapshot_nodes",
        "snapshot_edges",
    ] {
        let mut statement = conn
            .prepare(&format!("SELECT metadata_json FROM {table}"))
            .expect("read metadata");
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .expect("rows");
        for row in rows {
            let value = row.expect("row");
            assert!(!value.contains("SYNTHETIC_SENTINEL"), "{table}: {value}");
            assert!(!value.contains("argv"), "{table}: {value}");
            if value != "{}" {
                let argc: i64 = conn
                    .query_row("SELECT json_extract(?1,'$.argc')", [&value], |row| {
                        row.get(0)
                    })
                    .expect("argc");
                assert_eq!(argc, 2);
                assert!(value.contains("retained"));
                assert!(value.contains("\"pid\":42"));
            }
        }
    }
    let report: String = conn
        .query_row(
            "SELECT report_json FROM test_runs WHERE id='legacy'",
            [],
            |row| row.get(0),
        )
        .expect("report");
    assert!(!report.contains("SYNTHETIC_SENTINEL"));
    assert!(!report.contains("argv"));
    assert!(report.contains("retained"));
    let run = store
        .latest_collector_run("docker")
        .expect("run")
        .expect("docker");
    assert_eq!(run.metadata_json, "{}");
    assert_eq!(
        run.error_message.as_deref(),
        Some("Docker read failed; legacy error detail removed")
    );
    store.initialize().expect("idempotent upgrade");
}

#[test]
fn v6_preserves_safe_counts_and_handles_invalid_argument_json() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("v5.db");
    let mut store = Store::open(&path).expect("store");
    store.apply_migrations_through(5).expect("v5");
    for (id, metadata) in [
        (
            "process:pid:1",
            r#"{"argv_json":"[\"api\",\"SYNTHETIC_SENTINEL\"]","pid":1}"#,
        ),
        (
            "process:pid:2",
            r#"{"argv_json":"SYNTHETIC_SENTINEL","argc":7,"pid":2}"#,
        ),
        ("process:pid:3", r#"{"argv":"SYNTHETIC_SENTINEL","pid":3}"#),
        (
            "process:pid:4",
            r#"{"argv_json":"{SYNTHETIC_SENTINEL","pid":4}"#,
        ),
        ("process:pid:5", "invalid SYNTHETIC_SENTINEL"),
    ] {
        let mut node = support::node_row(id, "process", "api", 1);
        node.metadata_json = metadata.into();
        store.upsert_node(&node).expect("node");
    }
    store.initialize().expect("upgrade");
    let conn = rusqlite::Connection::open(&path).expect("connection");
    for (pid, argc) in [(1, Some(2)), (2, Some(7)), (3, None), (4, None), (5, None)] {
        let metadata = store
            .get_node(&format!("process:pid:{pid}"))
            .expect("read")
            .expect("node")
            .metadata_json;
        assert!(!metadata.contains("SYNTHETIC_SENTINEL"));
        assert!(!metadata.contains("argv"));
        let count: Option<i64> = conn
            .query_row("SELECT json_extract(?1,'$.argc')", [&metadata], |row| {
                row.get(0)
            })
            .expect("argc");
        assert_eq!(count, argc);
        if pid == 5 {
            assert_eq!(metadata, "{}");
        }
    }
}

#[test]
fn failed_v6_rolls_back_cleanup_and_version_marker() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("v5.db");
    let mut store = Store::open(&path).expect("store");
    store.apply_migrations_through(5).expect("v5");
    let mut node = support::node_row("process:pid:42", "process", "api", 1);
    node.metadata_json = r#"{"argv":["SYNTHETIC_SENTINEL"]}"#.into();
    store.upsert_node(&node).expect("legacy node");
    let conn = rusqlite::Connection::open(&path).expect("connection");
    conn.execute("DROP TABLE edge_history", [])
        .expect("break schema fixture");
    assert!(store.initialize().is_err());
    assert_eq!(store.schema_version().expect("version"), 5);
    assert_eq!(
        store
            .get_node("process:pid:42")
            .expect("read")
            .expect("node")
            .metadata_json,
        node.metadata_json
    );
}
