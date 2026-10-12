use std::path::Path;
use twin_core::RiskLevel;
use twin_test::{
    evaluate, parse_str, CheckStatus, EdgeFact, EmulationFact, EndpointFact, GraphFacts,
    ServiceFact, UnknownFact,
};

fn suite(checks: &str, strict: bool) -> twin_test::TestDocument {
    parse_str(
        &format!(
            "version: 1\nname: trust\ndefaults:\n  fail_on_unknown: {strict}\nchecks:\n{checks}"
        ),
        Path::new("twin.yaml"),
    )
    .unwrap()
}

#[test]
fn explanation_is_not_supporting_evidence() {
    let document = suite("  - name: evidence required\n    emulate:\n      action: restart\n      target: service:db.service\n    expect:\n      max_risk: high\n      require_evidence: true\n", false);
    let mut facts = GraphFacts {
        emulations: vec![EmulationFact {
            action: "restart".into(),
            target: "service:db.service".into(),
            risk: RiskLevel::Low,
            evidence_label: "weak".into(),
            evidence: vec!["no supporting observations".into()],
            supporting_observations: 0,
        }],
        ..GraphFacts::default()
    };
    assert_eq!(evaluate(&document, &facts).failed, 1);
    facts.emulations[0].supporting_observations = 1;
    assert_eq!(evaluate(&document, &facts).passed, 1);
}

#[test]
fn incomplete_coverage_cannot_prove_absence_or_outbound_compliance() {
    let checks = "  - name: absent node\n    assert:\n      node: process:pid:9\n      exists: false\n  - name: absent dependency\n    assert:\n      dependency:\n        from: service:a.service\n        to: service:b.service\n      exists: false\n  - name: outbound\n    assert:\n      outbound_endpoints:\n        allowed: []\n        fail_on_unknown: true\n";
    let facts = GraphFacts {
        unknowns: vec![UnknownFact {
            source: None,
            kind: "unavailable_collector".into(),
            detail: "TCP table unavailable".into(),
        }],
        ..GraphFacts::default()
    };
    let report = evaluate(&suite(checks, false), &facts);
    assert_eq!((report.passed, report.warned, report.failed), (0, 2, 1));
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:"));
    assert_eq!(evaluate(&suite(checks, true), &facts).failed, 3);
    assert_eq!(
        evaluate(&suite(checks, true), &GraphFacts::default()).passed,
        3
    );
}

#[test]
fn outbound_transport_and_workload_scope_are_preserved() {
    let document = suite("  - name: api policy\n    assert:\n      outbound_endpoints:\n        from: service:api.service\n        allowed: ['tcp:127.0.0.1:443']\n        fail_on_unknown: true\n", false);
    let mut facts = GraphFacts {
        endpoints: vec![
            EndpointFact {
                endpoint: "tcp:127.0.0.1:443".into(),
                from: "service:api.service".into(),
                ..EndpointFact::default()
            },
            EndpointFact {
                endpoint: "udp:10.0.0.1:53".into(),
                from: "service:other.service".into(),
                ..EndpointFact::default()
            },
        ],
        ..GraphFacts::default()
    };
    facts.nodes.push(twin_test::NodeFact {
        id: "service:api.service".into(),
        stale: false,
    });
    assert_eq!(evaluate(&document, &facts).passed, 1);
    facts.endpoints[0].endpoint = "udp:127.0.0.1:443".into();
    assert_eq!(evaluate(&document, &facts).failed, 1);
    let legacy = suite("  - name: any transport\n    assert:\n      outbound_endpoints:\n        allowed: ['127.0.0.1:443', '10.0.0.1:53']\n        fail_on_unknown: true\n", false);
    assert_eq!(evaluate(&legacy, &facts).passed, 1);
}

#[test]
fn health_contracts_require_runtime_state_and_owned_local_listener() {
    let document = suite("  - name: active\n    assert:\n      service: service:db.service\n      active_state: active\n  - name: listener\n    assert:\n      listener:\n        port: port:tcp:127.0.0.1:5432\n        owned_by: service:db.service\n", true);
    let mut facts = GraphFacts::default();
    assert_eq!(evaluate(&document, &facts).failed, 2);
    facts.services.push(ServiceFact {
        id: "service:db.service".into(),
        active_state: Some("active".into()),
        stale: false,
    });
    facts.edges.push(EdgeFact {
        from: "service:db.service".into(),
        to: "port:tcp:127.0.0.1:5432".into(),
        kind: "connects_to".into(),
        evidence: twin_core::EvidenceLabel::Strong,
        supporting_observations: 1,
        stale: false,
    });
    assert_eq!(
        evaluate(&document, &facts).checks[1].status,
        CheckStatus::Fail
    );
    facts.edges[0].kind = "listens_on".into();
    assert_eq!(evaluate(&document, &facts).passed, 2);
    facts.services[0].stale = true;
    assert_eq!(
        evaluate(&document, &facts).checks[0].status,
        CheckStatus::Fail
    );
}

#[test]
fn container_and_kubernetes_contracts_distinguish_unknown_unhealthy_and_routed_readiness() {
    let document = suite("  - name: container\n    assert:\n      container:\n        id: container:api\n        healthy: true\n  - name: routed deployment\n    assert:\n      k8s_ready:\n        deployment: k8s:deployment:default/api\n        routed_service: k8s:service:default/api\n", true);
    let mut facts = GraphFacts::default();
    assert_eq!(evaluate(&document, &facts).failed, 2);
    facts.containers.push(twin_test::ContainerFact {
        id: "container:api".into(),
        is_running: Some(true),
        is_healthy: Some(false),
        stale: false,
    });
    facts.workloads.push(twin_test::K8sReadyFact {
        id: "k8s:deployment:default/api".into(),
        ready: Some(2),
        desired: Some(2),
        is_current_generation: Some(true),
        stale: false,
    });
    facts.routing.push(twin_test::RoutingFact {
        id: "k8s:service:default/api".into(),
        ready_workloads: vec!["k8s:deployment:default/api".into(); 2],
        is_known: false,
        stale: false,
    });
    assert_eq!(evaluate(&document, &facts).failed, 2);
    facts.containers[0].is_healthy = Some(true);
    facts.routing[0].is_known = true;
    assert_eq!(evaluate(&document, &facts).passed, 2);
    facts.workloads[0].is_current_generation = Some(false);
    assert_eq!(evaluate(&document, &facts).failed, 1);
    facts.workloads[0].is_current_generation = Some(true);
    facts.routing[0].ready_workloads.pop();
    assert_eq!(evaluate(&document, &facts).failed, 1);
}

#[test]
fn listener_without_retained_observations_is_unknown() {
    let document=suite("  - name: listener\n    assert:\n      listener:\n        port: port:tcp:127.0.0.1:5432\n        owned_by: service:db.service\n",true);
    let facts = GraphFacts {
        edges: vec![EdgeFact {
            from: "service:db.service".into(),
            to: "port:tcp:127.0.0.1:5432".into(),
            kind: "listens_on".into(),
            evidence: twin_core::EvidenceLabel::Strong,
            supporting_observations: 0,
            stale: false,
        }],
        ..GraphFacts::default()
    };
    let report = evaluate(&document, &facts);
    assert_eq!(report.failed, 1);
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:"));
}
