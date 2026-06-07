use twin_core::{EdgeClass, EdgeId, EdgeKind, EvidenceStrength, NodeId};
use twin_emulate::{
    emulate_restart_service, EmulationDependent, EmulationEvidenceLine, EmulationImpactPath,
    EmulationNode, EmulationPathStep, OverlayNodeState, RestartServiceInput, SAFETY_STATEMENT,
};

fn postgres_target() -> EmulationNode {
    EmulationNode {
        id: NodeId::service("postgresql.service"),
        label: "postgresql.service".to_string(),
    }
}

fn django_dependent() -> EmulationDependent {
    let target = NodeId::service("postgresql.service");
    let django = NodeId::service("django.service");
    EmulationDependent {
        id: django.clone(),
        label: "django.service".to_string(),
        edge_kind: EdgeKind::DependsOn,
        edge_class: EdgeClass::Inferred,
        reason: "active connection".to_string(),
        path: vec![EmulationPathStep {
            from_id: django,
            from_label: "django.service".to_string(),
            edge_kind: EdgeKind::DependsOn,
            edge_class: EdgeClass::Inferred,
            to_id: target,
            to_label: "postgresql.service".to_string(),
            edge_id: EdgeId::new(
                &NodeId::service("django.service"),
                EdgeKind::DependsOn,
                &NodeId::service("postgresql.service"),
            ),
        }],
        evidence: vec![],
        observation_ids: vec!["obs-1".to_string()],
    }
}

#[test]
fn restart_marks_service_and_sockets_unavailable() {
    let input = RestartServiceInput {
        target: postgres_target(),
        unavailable_nodes: vec![EmulationNode {
            id: NodeId::port_tcp("127.0.0.1", 5432).expect("port"),
            label: "127.0.0.1:5432".to_string(),
        }],
        runtime_dependents: vec![django_dependent()],
        ..RestartServiceInput::default()
    };
    let report = emulate_restart_service(input);
    assert!(!report.action_performed);
    assert_eq!(report.safety_statement, SAFETY_STATEMENT);
    assert_eq!(report.overlay.node_overrides.len(), 2);
    assert!(report
        .overlay
        .node_overrides
        .iter()
        .all(|n| n.state == OverlayNodeState::TemporarilyUnavailable));
    assert_eq!(report.transient_impacts.len(), 1);
    assert_eq!(report.overlay.interrupted_relationships.len(), 1);
}

#[test]
fn restart_keeps_configured_dependents_separate() {
    let target = NodeId::service("postgresql.service");
    let backup = NodeId::service("backup.service");
    let input = RestartServiceInput {
        target: postgres_target(),
        configured_dependents: vec![EmulationDependent {
            id: backup.clone(),
            label: "backup.service".to_string(),
            edge_kind: EdgeKind::DependsOn,
            edge_class: EdgeClass::Observed,
            reason: "Requires=postgresql.service".to_string(),
            path: vec![EmulationPathStep {
                from_id: backup,
                from_label: "backup.service".to_string(),
                edge_kind: EdgeKind::DependsOn,
                edge_class: EdgeClass::Observed,
                to_id: target,
                to_label: "postgresql.service".to_string(),
                edge_id: EdgeId::new(
                    &NodeId::service("backup.service"),
                    EdgeKind::DependsOn,
                    &NodeId::service("postgresql.service"),
                ),
            }],
            evidence: vec![],
            observation_ids: vec!["obs-2".to_string()],
        }],
        ..RestartServiceInput::default()
    };
    let report = emulate_restart_service(input);
    assert!(report.transient_impacts.is_empty());
    assert_eq!(report.configured_impacts.len(), 1);
    assert_eq!(report.risk_level, twin_core::RiskLevel::Low);
}

#[test]
fn direct_only_does_not_include_transitive_dependents() {
    let target = NodeId::service("postgresql.service");
    let api = NodeId::service("api.service");
    let frontend = NodeId::service("frontend.service");
    let input = RestartServiceInput {
        target: postgres_target(),
        runtime_dependents: vec![EmulationDependent {
            id: api.clone(),
            label: "api.service".to_string(),
            edge_kind: EdgeKind::DependsOn,
            edge_class: EdgeClass::Inferred,
            reason: "active connection".to_string(),
            path: vec![EmulationPathStep {
                from_id: api,
                from_label: "api.service".to_string(),
                edge_kind: EdgeKind::DependsOn,
                edge_class: EdgeClass::Inferred,
                to_id: target.clone(),
                to_label: "postgresql.service".to_string(),
                edge_id: EdgeId::new(
                    &NodeId::service("api.service"),
                    EdgeKind::DependsOn,
                    &target,
                ),
            }],
            evidence: vec![],
            observation_ids: vec![],
        }],
        ..RestartServiceInput::default()
    };
    let report = emulate_restart_service(input);
    assert_eq!(report.transient_impacts.len(), 1);
    assert!(!report
        .transient_impacts
        .iter()
        .any(|i| i.id == frontend.to_string()));
}

#[test]
fn restart_report_action_performed_is_false() {
    let report = emulate_restart_service(RestartServiceInput {
        target: postgres_target(),
        ..RestartServiceInput::default()
    });
    assert!(!report.action_performed);
}

#[test]
fn restart_with_paths_includes_path_evidence_in_strength() {
    let nginx = NodeId::service("nginx.service");
    let path_evidence = EmulationEvidenceLine {
        source: "/proc/net/tcp:1".to_string(),
        statement: "Observed: active connection".to_string(),
        relationship: "inferred".to_string(),
        strength_score: 95,
    };
    let mut dependent = django_dependent();
    dependent.evidence = vec![EmulationEvidenceLine {
        source: "/proc/net/tcp:2".to_string(),
        statement: "Observed: direct connection".to_string(),
        relationship: "inferred".to_string(),
        strength_score: 70,
    }];
    let base = RestartServiceInput {
        target: postgres_target(),
        runtime_dependents: vec![dependent],
        ..RestartServiceInput::default()
    };
    let without_paths = emulate_restart_service(base.clone());
    let with_paths = emulate_restart_service(RestartServiceInput {
        paths_requested: true,
        impact_paths: vec![EmulationImpactPath {
            terminal_id: nginx,
            terminal_label: "nginx.service".to_string(),
            depth: 2,
            steps: vec![],
            evidence: vec![path_evidence],
            is_cycle_capped: false,
            is_depth_capped: false,
            cycle_note: None,
        }],
        ..base
    });
    assert!(
        with_paths.evidence_strength.score() > without_paths.evidence_strength.score(),
        "path evidence should raise evidence strength"
    );
    assert_eq!(with_paths.impact_paths[0].evidence.len(), 1);
}
