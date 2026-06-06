use twin_core::{EdgeClass, EdgeId, EdgeKind, NodeId};
use twin_emulate::{
    emulate_restart_service, EmulationDependent, EmulationNode, EmulationPathStep,
    OverlayNodeState, RestartServiceInput, SAFETY_STATEMENT,
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
        configured_dependents: vec![],
        unknowns: vec![],
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
        unavailable_nodes: vec![],
        runtime_dependents: vec![],
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
        unknowns: vec![],
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
        unavailable_nodes: vec![],
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
        configured_dependents: vec![],
        unknowns: vec![],
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
        unavailable_nodes: vec![],
        runtime_dependents: vec![],
        configured_dependents: vec![],
        unknowns: vec![],
    });
    assert!(!report.action_performed);
}
