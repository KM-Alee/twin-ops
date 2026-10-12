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

struct FixtureDns {
    answers: twin_collectors::dns::DnsAnswers,
    expected: std::collections::BTreeSet<String>,
}

impl twin_collectors::dns::DnsReader for FixtureDns {
    fn resolve(
        &self,
        hosts: &std::collections::BTreeSet<String>,
    ) -> twin_collectors::dns::DnsAnswers {
        assert_eq!(hosts, &self.expected);
        self.answers.clone()
    }
}

#[test]
fn outbound_dns_matches_both_address_families_and_udp_with_sourced_inference() {
    use std::collections::{BTreeMap, BTreeSet};
    use twin_core::TransportProtocol;
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let seen = TimestampNs::new(1);
    let process = NodeId::process(42);
    store
        .upsert_node_typed(&GraphNode::process(42, "api", seen, None))
        .expect("process");
    for (protocol, ip, port) in [
        (TransportProtocol::Tcp, "10.0.0.8", 443),
        (TransportProtocol::Udp, "2001:db8::8", 443),
    ] {
        let target = NodeId::inet_port(protocol, ip, port).expect("id");
        store
            .upsert_node_typed(&GraphNode::inet_port(protocol, ip, port, seen, None).expect("node"))
            .expect("port");
        store
            .upsert_edge_typed(&GraphEdge::observed_process_connects_to(
                &process, &target, seen, None,
            ))
            .expect("edge");
    }
    store
        .insert_collector_run(&twin_store::CollectorRunRow {
            id: None,
            collector: twin_collectors::COLLECTOR_NAME.into(),
            started_at_ns: 1,
            ended_at_ns: TimestampNs::now().as_i64(),
            status: "success".into(),
            observation_count: 2,
            warning_count: 0,
            error_message: None,
            metadata_json: "{}".into(),
        })
        .unwrap();
    drop(store);
    let path = home.layout.db_file().with_extension("yaml");
    std::fs::write(
        &path,
        r#"
version: 1
name: dns
checks:
  - name: api
    assert:
      outbound_endpoints:
        allowed: ['API.Example.:443', '127.0.0.1:5432']
        fail_on_unknown: true
  - name: duplicate hostname
    assert:
      outbound_endpoints:
        allowed: ['api.example:443']
        fail_on_unknown: true
"#,
    )
    .expect("suite");
    let reader = FixtureDns {
        expected: BTreeSet::from(["api.example".into()]),
        answers: BTreeMap::from([(
            "api.example".into(),
            Ok(vec![
                "10.0.0.8".parse().unwrap(),
                "2001:db8::8".parse().unwrap(),
            ]),
        )]),
    };
    let report = twin_app::run_file_with_dns(&home.layout, &path, &reader).expect("run");
    assert_eq!(report.passed, 2);
    assert_eq!(report.failed, 0);
    let evidence = report.checks[0].evidence.as_deref().unwrap();
    for text in [
        "10.0.0.8:443",
        "[2001:db8::8]:443",
        "timestamp_ns=",
        "inferred",
        "historical hostname ownership unknown",
    ] {
        assert!(evidence.contains(text), "{evidence}");
    }
    let stored = Store::open(&home.layout.db_file())
        .unwrap()
        .list_test_runs()
        .unwrap();
    assert!(stored[0]
        .report_json
        .contains("historical hostname ownership unknown"));
}

#[test]
fn outbound_dns_failure_never_passes_even_without_observed_endpoints() {
    use std::collections::{BTreeMap, BTreeSet};
    use twin_collectors::dns::DnsError;
    for error in [
        DnsError::Timeout,
        DnsError::Unavailable,
        DnsError::Limit,
        DnsError::Empty,
    ] {
        let home = support::IsolatedHome::new();
        init_in(&home.layout, InitRequest::default()).unwrap();
        let path = home.layout.db_file().with_extension("yaml");
        std::fs::write(
            &path,
            r#"
version: 1
name: dns
checks:
  - name: strict
    assert:
      outbound_endpoints:
        allowed: ['missing.example:443']
        fail_on_unknown: true
  - name: advisory
    assert:
      outbound_endpoints:
        allowed: ['missing.example:443']
"#,
        )
        .unwrap();
        let reader = FixtureDns {
            expected: BTreeSet::from(["missing.example".into()]),
            answers: BTreeMap::from([("missing.example".into(), Err(error))]),
        };
        let report = twin_app::run_file_with_dns(&home.layout, &path, &reader).unwrap();
        assert_eq!((report.passed, report.warned, report.failed), (0, 1, 1));
        assert_eq!(report.checks[0].status, CheckStatus::Fail);
        assert_eq!(report.checks[1].status, CheckStatus::Warn);
        assert!(report.checks[0]
            .detail
            .as_ref()
            .unwrap()
            .contains(&error.to_string()));
    }
}

#[test]
fn malformed_dns_entries_are_not_queried_or_copied_into_reports() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).unwrap();
    let path = home.layout.db_file().with_extension("yaml");
    std::fs::write(
        &path,
        r#"
version: 1
name: invalid
checks:
  - name: outbound
    assert:
      outbound_endpoints:
        allowed: ['https://user:SECRET@api.example:443', 'api.example:99999']
        fail_on_unknown: true
"#,
    )
    .unwrap();
    let reader = FixtureDns {
        expected: Default::default(),
        answers: Default::default(),
    };
    let report = twin_app::run_file_with_dns(&home.layout, &path, &reader).unwrap();
    assert_eq!(report.failed, 1);
    assert!(!serde_json::to_string(&report).unwrap().contains("SECRET"));
}
