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
