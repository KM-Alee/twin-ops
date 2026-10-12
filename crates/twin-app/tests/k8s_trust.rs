mod support;

use support::IsolatedHome;
use twin_app::{InitRequest, K8sScanRequest};
use twin_store::{CollectorRunRow, Store};

fn fixture(home: &IsolatedHome, uid: &str) -> std::path::PathBuf {
    let root = home.layout.data_dir.join("k8s-trust");
    std::fs::create_dir_all(&root).expect("fixture");
    std::fs::write(root.join("pods.json"), format!(r#"{{"kind":"Pod","metadata":{{"namespace":"default","name":"api","uid":"{uid}"}},"spec":{{"containers":[{{"name":"api","image":"api:1"}}]}},"status":{{"conditions":[{{"type":"Ready","status":"True"}}]}}}}"#)).expect("pod");
    root
}

#[test]
fn cluster_source_switch_is_rejected_before_graph_or_history_writes() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = fixture(&home, "original");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "k8s".into(),
            started_at_ns: 1,
            ended_at_ns: 1,
            status: "success".into(),
            observation_count: 0,
            warning_count: 0,
            error_message: None,
            metadata_json: r#"{"source_identity":"other-cluster"}"#.into(),
        })
        .expect("binding");
    let error = twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(root),
            config_override: None,
        },
    )
    .expect_err("different source");
    assert!(error.to_string().contains("separate XDG_DATA_HOME"));
    assert_eq!(store.count_nodes().expect("nodes"), 0);
    assert_eq!(
        store.list_collector_runs(Some("k8s")).expect("runs").len(),
        1
    );
}

#[test]
fn recreated_pod_uid_resets_lifetime_and_retains_generation_evidence() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = fixture(&home, "original");
    let request = K8sScanRequest {
        fixture: Some(root),
        config_override: None,
    };
    twin_app::k8s_scan_in(&home.layout, request.clone()).expect("first");
    let store = Store::open(&home.layout.db_file()).expect("store");
    let first = store
        .get_node("k8s:pod:default/api")
        .expect("query")
        .expect("pod");
    fixture(&home, "replacement");
    twin_app::k8s_scan_in(&home.layout, request).expect("replacement");
    let replaced = store
        .get_node("k8s:pod:default/api")
        .expect("query")
        .expect("pod");
    assert!(replaced.first_seen_ns > first.first_seen_ns);
    assert!(replaced.metadata_json.contains("replacement"));
}

#[test]
fn mid_scan_storage_failure_rolls_back_graph_history_and_collector_run() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = fixture(&home, "original");
    std::fs::write(root.join("pods.json"), r#"[{"kind":"Pod","metadata":{"namespace":"default","name":"a"}},{"kind":"Pod","metadata":{"namespace":"default","name":"z"}}]"#).expect("pods");
    let connection = rusqlite::Connection::open(home.layout.db_file()).expect("connection");
    connection.execute_batch("CREATE TRIGGER fail_second_pod BEFORE INSERT ON nodes WHEN NEW.id = 'k8s:pod:default/z' BEGIN SELECT RAISE(ABORT, 'injected failure'); END;").expect("trigger");
    twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(root),
            config_override: None,
        },
    )
    .expect_err("failure");
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert_eq!(store.count_nodes().expect("nodes"), 0);
    assert!(store
        .list_collector_runs(Some("k8s"))
        .expect("runs")
        .is_empty());
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM node_history", [], |row| row.get(0))
        .expect("history");
    assert_eq!(count, 0);
}

#[test]
fn selector_ready_pods_do_not_fabricate_endpoint_membership() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let root = fixture(&home, "original");
    std::fs::write(root.join("service.json"), r#"{"kind":"Service","metadata":{"name":"api","namespace":"default"},"spec":{"selector":{"app":"api"}}}"#).expect("service");
    twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(root.clone()),
            config_override: None,
        },
    )
    .expect("scan");
    let store = Store::open(&home.layout.db_file()).expect("store");
    let row = store
        .get_node("k8s:service:default/api")
        .expect("query")
        .expect("service");
    let metadata: serde_json::Value = serde_json::from_str(&row.metadata_json).expect("metadata");
    assert_eq!(metadata["ready_pods"], serde_json::json!([]));
    assert_eq!(metadata["ready_pods_known"], true);
    std::fs::write(root.join("broken.json"), "[").expect("broken");
    twin_app::k8s_scan_in(
        &home.layout,
        K8sScanRequest {
            fixture: Some(root),
            config_override: None,
        },
    )
    .expect("partial scan");
    let row = store
        .get_node("k8s:service:default/api")
        .expect("query")
        .expect("service");
    let metadata: serde_json::Value = serde_json::from_str(&row.metadata_json).expect("metadata");
    assert_eq!(metadata["ready_pods_known"], false);
}
