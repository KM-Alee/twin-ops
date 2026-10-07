use std::path::Path;

use twin_core::EvidenceLabel;
use twin_test::{evaluate, parse_str, CheckStatus, EdgeFact, GraphFacts, NodeFact};

fn facts() -> GraphFacts {
    GraphFacts {
        nodes: vec![
            NodeFact {
                id: "service:django.service".to_string(),
                stale: false,
            },
            NodeFact {
                id: "port:tcp:127.0.0.1:5432".to_string(),
                stale: true,
            },
        ],
        edges: vec![EdgeFact {
            from: "service:django.service".to_string(),
            to: "service:postgresql.service".to_string(),
            kind: "depends_on".to_string(),
            evidence: EvidenceLabel::Weak,
            stale: false,
        }],
        ..GraphFacts::default()
    }
}

#[test]
fn node_port_service_and_dependency_checks_report_pass_warn_fail() {
    let yaml = r#"
version: 1
name: local-readiness
checks:
  - name: django service exists
    assert:
      service: service:django.service
      exists: true
  - name: postgres port is stale
    assert:
      port: port:tcp:127.0.0.1:5432
      exists: true
  - name: missing file
    assert:
      node: file:/etc/nginx/nginx.conf
      exists: true
  - name: django depends on postgres
    assert:
      dependency:
        from: service:django.service
        to: service:postgresql.service
      min_evidence: moderate
"#;
    let doc = parse_str(yaml, Path::new("twin.yaml")).expect("parse");
    let report = evaluate(&doc, &facts());
    assert_eq!(report.checks[0].status, CheckStatus::Pass);
    assert_eq!(report.checks[1].status, CheckStatus::Warn);
    assert_eq!(report.checks[2].status, CheckStatus::Fail);
    assert_eq!(report.checks[3].status, CheckStatus::Fail);
    assert!(report.checks[3]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("minimum is moderate"));
    assert_eq!(report.passed, 1);
    assert_eq!(report.warned, 1);
    assert_eq!(report.failed, 2);
}

#[test]
fn emulation_endpoint_disk_and_unknowns_checks() {
    use twin_core::RiskLevel;
    use twin_test::{DiskFact, EmulationFact, EndpointFact, UnknownFact};

    let yaml = r#"
version: 1
name: local-readiness
checks:
  - name: postgres restart risk acceptable
    emulate:
      action: restart
      target: service:postgresql.service
    expect:
      max_risk: high
      require_evidence: true
  - name: no unexpected outbound endpoints
    assert:
      outbound_endpoints:
        allowed:
          - 127.0.0.1:5432
        fail_on_unknown: true
  - name: root disk under limit
    assert:
      disk:
        mount: /
        max_used_percent: 85
  - name: no unmapped sockets
    assert:
      unknowns:
        max_count: 0
"#;
    let doc = parse_str(yaml, Path::new("twin.yaml")).expect("parse");
    let facts = GraphFacts {
        endpoints: vec![EndpointFact {
            endpoint: "telemetry.example.com:443".to_string(),
            evidence: "eBPF observed service:api.service connecting 42 times".to_string(),
        }],
        disks: vec![DiskFact {
            mount: "/".to_string(),
            used_percent: 90,
        }],
        unknowns: vec![UnknownFact {
            kind: "unmapped_sockets".to_string(),
            detail: "3 sockets could not be mapped".to_string(),
        }],
        emulations: vec![EmulationFact {
            action: "restart".to_string(),
            target: "service:postgresql.service".to_string(),
            risk: RiskLevel::High,
            evidence_label: "strong".to_string(),
            evidence: vec!["eBPF connect observed".to_string()],
        }],
        ..GraphFacts::default()
    };
    let report = evaluate(&doc, &facts);
    assert_eq!(report.checks[0].status, CheckStatus::Warn);
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("HIGH"));
    assert!(report.checks[0]
        .evidence
        .as_deref()
        .unwrap_or("")
        .contains("STRONG"));
    assert_eq!(report.checks[1].status, CheckStatus::Fail);
    assert!(report.checks[1]
        .detail
        .as_deref()
        .unwrap_or("")
        .contains("telemetry.example.com:443"));
    assert!(report.checks[1]
        .evidence
        .as_deref()
        .unwrap_or("")
        .contains("eBPF observed"));
    assert_eq!(report.checks[2].status, CheckStatus::Fail);
    assert_eq!(report.checks[3].status, CheckStatus::Fail);
    assert!(report.checks[3]
        .evidence
        .as_deref()
        .unwrap_or("")
        .contains("could not be mapped"));
}
