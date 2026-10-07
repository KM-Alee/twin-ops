mod support;

use twin_app::{init_file, init_in, lint_file, run_file, InitRequest, TestInitRequest};
use twin_core::{GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_store::Store;
use twin_test::CheckStatus;

#[test]
fn run_persists_pass_and_fail_against_the_stored_graph() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let seen = TimestampNs::new(1);
    store
        .upsert_node_typed(&GraphNode::service("django.service", seen, None))
        .expect("django");
    store
        .upsert_node_typed(&GraphNode::service("postgresql.service", seen, None))
        .expect("postgres");
    store
        .upsert_edge_typed(&GraphEdge::inferred_service_depends_on(
            &NodeId::service("django.service"),
            &NodeId::service("postgresql.service"),
            seen,
            None,
        ))
        .expect("edge");
    drop(store);

    let path = home
        .layout
        .db_file()
        .parent()
        .expect("parent")
        .join("suite.yaml");
    let yaml = r#"
version: 1
name: local-readiness
checks:
  - name: django service exists
    assert:
      service: service:django.service
      exists: true
  - name: postgres is listening
    assert:
      port: port:tcp:127.0.0.1:5432
      exists: true
  - name: django depends on postgres
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: strong
"#;
    std::fs::write(&path, yaml).expect("yaml");
    let document = lint_file(&path).expect("lint");
    assert_eq!(document.name, "local-readiness");
    let report = run_file(&home.layout, &path).expect("run");
    assert_eq!(report.checks[0].status, CheckStatus::Pass);
    assert_eq!(report.checks[1].status, CheckStatus::Fail);
    assert_eq!(report.checks[2].status, CheckStatus::Fail);
    assert!(report.checks[2]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("minimum is strong"));
    let store = Store::open(&home.layout.db_file()).expect("store");
    let runs = store.list_test_runs().expect("runs");
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].name, "local-readiness");
    assert_eq!(runs[0].failed, 2);
    assert!(runs[0].report_json.contains("django service exists"));
}

#[test]
fn init_writes_a_file_that_lints() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("nested").join("twin.yaml");
    init_file(&TestInitRequest {
        path: path.clone(),
        force: false,
    })
    .expect("init");
    let document = lint_file(&path).expect("lint");
    assert_eq!(document.checks.len(), 8);
}

#[test]
fn run_calls_emulation_and_fails_unknown_endpoints_with_evidence() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let seen = TimestampNs::new(1);
    let process = NodeId::process(4421);
    let port = NodeId::port_tcp("10.0.0.8", 443).expect("port");
    store
        .upsert_node_typed(&GraphNode::service("django.service", seen, None))
        .expect("service");
    store
        .upsert_node_typed(&GraphNode::process(4421, "django", seen, None))
        .expect("process");
    store
        .upsert_node_typed(&GraphNode::tcp_port("10.0.0.8", 443, seen, None).expect("port node"))
        .expect("port");
    store
        .upsert_edge_typed(&GraphEdge::observed_process_connects_to(
            &process, &port, seen, None,
        ))
        .expect("connect");
    drop(store);

    let path = home
        .layout
        .db_file()
        .parent()
        .expect("parent")
        .join("ops.yaml");
    let yaml = r#"
version: 1
name: local-readiness
checks:
  - name: django restart risk acceptable
    emulate:
      action: restart
      target: service:django.service
    expect:
      max_risk: critical
      require_evidence: false
  - name: no unexpected outbound endpoints
    assert:
      outbound_endpoints:
        allowed:
          - 127.0.0.1:5432
        fail_on_unknown: true
"#;
    std::fs::write(&path, yaml).expect("yaml");
    let report = run_file(&home.layout, &path).expect("run");
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("Risk:"));
    assert_eq!(report.checks[1].status, CheckStatus::Fail);
    assert!(report.checks[1]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("10.0.0.8:443"));
    assert!(report.checks[1]
        .evidence
        .as_deref()
        .unwrap_or("")
        .contains("socket table observed"));
}
