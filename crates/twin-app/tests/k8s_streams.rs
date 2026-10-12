mod support;

use twin_app::{InitRequest, K8sLogsRequest, K8sWatchRequest, PodLogRequest};
use twin_k8s::ClientConfig;
use twin_store::{NodeRow, Store};

fn server(mode: &str) -> Option<twin_fixtures::K8sStreamFixture> {
    match twin_fixtures::K8sStreamFixture::new(mode) {
        Ok(server) => Some(server),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("runtime capability unavailable: local TCP fixture bind denied; skipping app stream API test");
            None
        }
        Err(error) => panic!("fixture: {error}"),
    }
}
fn config(server: &twin_fixtures::K8sStreamFixture) -> ClientConfig {
    ClientConfig::new(server.url().parse().expect("url"))
}

#[test]
fn watch_refreshes_graph_and_records_interruption_coverage_without_private_data() {
    let Some(server) = server("logs") else {
        return;
    };
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    store
        .upsert_node(&NodeRow {
            id: "k8s:pod:default/obsolete".into(),
            kind: "k8s_pod".into(),
            label: "obsolete".into(),
            state: "active".into(),
            first_seen_ns: 1,
            last_seen_ns: 1,
            valid_from_ns: 1,
            valid_to_ns: None,
            metadata_json: r#"{"source":"k8s"}"#.into(),
        })
        .expect("old pod");
    let result = twin_app::k8s_watch_configured_in(
        &home.layout,
        &K8sWatchRequest {
            duration_seconds: 5,
            max_events: 2,
            config_override: None,
        },
        config(&server),
    )
    .expect("watch");
    assert_eq!(result.summary.observed_events, 2);
    assert!(result.summary.refreshes > 1);
    assert!(!result.summary.warnings.is_empty());
    assert_eq!(
        store
            .get_node("k8s:pod:default/obsolete")
            .expect("read")
            .expect("pod")
            .state,
        "gone"
    );
    let runs = store.list_collector_runs(Some("k8s")).expect("runs");
    assert!(runs.iter().any(|run| run.status == "unavailable"));
    assert!(runs.iter().any(|run| run.status == "success"));
    assert!(!serde_json::to_string(&result)
        .expect("output")
        .contains("PRIVATE_"));
    assert!(runs
        .iter()
        .all(|run| !format!("{run:?}").contains("PRIVATE_")));
}

#[test]
fn pod_log_summary_leaves_graph_and_observation_store_unchanged() {
    let Some(server) = server("logs") else {
        return;
    };
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let store = Store::open(&home.layout.db_file()).expect("store");
    let before = (
        store.count_nodes().expect("nodes"),
        store.count_observations().expect("observations"),
        store.list_collector_runs(None).expect("runs"),
    );
    let request = K8sLogsRequest {
        pod: PodLogRequest {
            namespace: "default".into(),
            pod: "api".into(),
            container: None,
            previous: false,
            tail_lines: 10,
            max_bytes: 1024,
            timeout_seconds: 1,
        },
        config_override: None,
    };
    let result =
        twin_app::k8s_logs_configured_in(&home.layout, &request, config(&server)).expect("logs");
    assert!(result.received_bytes > 0);
    assert!(!serde_json::to_string(&result)
        .expect("output")
        .contains("PRIVATE_"));
    assert_eq!(
        (
            store.count_nodes().expect("nodes"),
            store.count_observations().expect("observations"),
            store.list_collector_runs(None).expect("runs")
        ),
        before
    );
}

#[test]
fn invalid_watch_request_does_not_write_collector_coverage() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    assert!(twin_app::k8s_watch_in(
        &home.layout,
        &K8sWatchRequest {
            duration_seconds: 0,
            max_events: 1,
            config_override: None
        }
    )
    .is_err());
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert!(store
        .list_collector_runs(Some("k8s"))
        .expect("runs")
        .is_empty());
}
