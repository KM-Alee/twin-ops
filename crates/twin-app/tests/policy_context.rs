mod support;

use twin_app::{InitRequest, TestRunOptions};
use twin_core::{GraphMetadata, GraphNode, NodeId, NodeKind, NodeState, TimestampNs};
use twin_store::{CollectorRunRow, Store};

fn state_node(state: &str, at: i64) -> GraphNode {
    GraphNode::from_parts(twin_core::GraphNodeParts {
        id: NodeId::service("api.service"),
        kind: NodeKind::Service,
        label: "api.service".into(),
        state: NodeState::Active,
        first_seen: TimestampNs::new(at),
        last_seen: TimestampNs::new(at),
        valid_from: TimestampNs::new(at),
        valid_to: None,
        metadata: GraphMetadata::from_json(format!(
            "{{\"active_state\":\"{state}\",\"runtime_observed_at_ns\":{at}}}"
        )),
    })
}

fn run_row(at: i64) -> CollectorRunRow {
    CollectorRunRow {
        id: None,
        collector: twin_collectors::COLLECTOR_NAME.into(),
        started_at_ns: at,
        ended_at_ns: at,
        status: "success".into(),
        observation_count: 1,
        warning_count: 0,
        error_message: None,
        metadata_json: "{}".into(),
    }
}

#[test]
fn snapshot_contract_baseline_reports_regression_without_using_current_state() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let at = TimestampNs::now().as_i64();
    store.upsert_node_typed(&state_node("active", at)).unwrap();
    store.insert_collector_run(&run_row(at)).unwrap();
    let mut runtime = run_row(at);
    runtime.collector = twin_collectors::RUNTIME_COLLECTOR_NAME.into();
    store.insert_collector_run(&runtime).unwrap();
    store.create_snapshot("healthy", at).unwrap();
    store
        .upsert_node_typed(&state_node("failed", at + 1))
        .unwrap();
    drop(store);
    let path = home.layout.data_dir.join("health.yaml");
    std::fs::write(&path, "version: 1\nname: health\nchecks:\n  - name: active api\n    assert:\n      service: service:api.service\n      active_state: active\n").unwrap();
    let current = twin_app::run_file_with_options(
        &home.layout,
        &path,
        &TestRunOptions {
            baseline: Some("healthy".into()),
            ..TestRunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(current.failed, 1);
    assert_eq!(current.baseline.unwrap().newly_failing, ["active api"]);
    let historical = twin_app::run_file_with_options(
        &home.layout,
        &path,
        &TestRunOptions {
            snapshot: Some("healthy".into()),
            ..TestRunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(historical.passed, 1);
    let context = historical.context.unwrap();
    assert_eq!(context.snapshot.as_deref(), Some("healthy"));
    assert!(context.live_sources.is_empty());
}

#[test]
fn explicit_max_age_rejects_old_runtime_health_and_unscanned_absence() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    store.upsert_node_typed(&state_node("active", 1)).unwrap();
    drop(store);
    let path = home.layout.data_dir.join("health.yaml");
    std::fs::write(&path, "version: 1\nname: health\ndefaults:\n  fail_on_unknown: true\nchecks:\n  - name: active api\n    assert:\n      service: service:api.service\n      active_state: active\n  - name: no hidden dependency\n    assert:\n      node: process:pid:999\n      exists: false\n").unwrap();
    let report = twin_app::run_file_with_options(
        &home.layout,
        &path,
        &TestRunOptions {
            max_age_seconds: Some(300),
            ..TestRunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(report.failed, 2);
    assert!(report.checks.iter().all(|check| check
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:")));
    assert_eq!(report.context.unwrap().max_age_seconds, Some(300));
}

#[test]
fn zero_observation_emulation_cannot_satisfy_evidence_required() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    store
        .upsert_node_typed(&GraphNode::service("api.service", TimestampNs::now(), None))
        .unwrap();
    drop(store);
    let path = home.layout.data_dir.join("evidence.yaml");
    std::fs::write(&path, "version: 1\nname: evidence\nchecks:\n  - name: restart\n    emulate:\n      action: restart\n      target: service:api.service\n    expect:\n      max_risk: high\n      require_evidence: true\n").unwrap();
    assert_eq!(twin_app::run_file(&home.layout, &path).unwrap().failed, 1);
}

#[test]
fn snapshot_listener_uses_captured_proof_after_reset_and_reports_pruning_as_unknown() {
    use twin_core::GraphEdge;
    use twin_store::ObservationRow;
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let at = TimestampNs::now();
    let service = NodeId::service("api.service");
    let port = NodeId::port_tcp("127.0.0.1", 8000).expect("port");
    store
        .upsert_node_typed(&state_node("active", at.as_i64()))
        .expect("service");
    store
        .upsert_node_typed(&GraphNode::tcp_port("127.0.0.1", 8000, at, None).expect("port node"))
        .expect("port");
    store
        .insert_collector_run(&run_row(at.as_i64()))
        .expect("coverage");
    let edge = GraphEdge::inferred_service_listens_on(&service, &port, at, None);
    store.upsert_edge_typed(&edge).expect("listener");
    let observation = ObservationRow {
        id: "00000000-0000-4000-8000-000000000042".into(),
        source: "proc_net_tcp".into(),
        kind: "tcp_socket_seen".into(),
        subject_node_id: Some(service.to_string()),
        object_node_id: Some(port.to_string()),
        timestamp_ns: at.as_i64(),
        confidence_hint: "high".into(),
        redaction_state: "none".into(),
        metadata_json: "{}".into(),
        collector_run_id: None,
    };
    store.insert_observation(&observation).expect("observation");
    store
        .link_edge_observation(edge.id().as_str(), &observation.id, "direct")
        .expect("link");
    store
        .create_snapshot("healthy", at.as_i64())
        .expect("snapshot");
    store
        .reset_edge_incarnation(edge.id(), TimestampNs::new(at.as_i64() + 1))
        .expect("reincarnation");
    let path = home.layout.data_dir.join("listener.yaml");
    std::fs::write(&path, "version: 1\nname: listener\ndefaults:\n  fail_on_unknown: true\nchecks:\n  - name: owned listener\n    assert:\n      listener:\n        port: port:tcp:127.0.0.1:8000\n        owned_by: service:api.service\n").expect("contract");
    let options = TestRunOptions {
        snapshot: Some("healthy".into()),
        ..TestRunOptions::default()
    };
    assert_eq!(
        twin_app::run_file_with_options(&home.layout, &path, &options)
            .expect("snapshot proof")
            .passed,
        1
    );
    store
        .prune_retention(at.as_i64() + 1, at.as_i64() + 1, 0)
        .expect("prune evidence");
    let pruned =
        twin_app::run_file_with_options(&home.layout, &path, &options).expect("pruned proof");
    assert_eq!(pruned.failed, 1);
    assert!(pruned.checks[0]
        .detail
        .as_deref()
        .expect("detail")
        .starts_with("Unknown:"));
}

#[test]
fn historical_emulation_checks_report_unknown() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    store
        .upsert_node_typed(&state_node("active", TimestampNs::now().as_i64()))
        .unwrap();
    store
        .create_snapshot("baseline", TimestampNs::now().as_i64())
        .unwrap();
    drop(store);
    let path = home.layout.data_dir.join("historical.yaml");
    std::fs::write(&path, "version: 1\nname: historical\nchecks:\n  - name: restart\n    emulate:\n      action: restart\n      target: service:api.service\n    expect:\n      max_risk: high\n      require_evidence: true\n").unwrap();
    let report = twin_app::run_file_with_options(
        &home.layout,
        &path,
        &TestRunOptions {
            snapshot: Some("baseline".into()),
            ..TestRunOptions::default()
        },
    )
    .unwrap();
    assert_eq!(report.warned, 1);
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:"));
    assert!(report.context.unwrap().live_sources.is_empty());
}
