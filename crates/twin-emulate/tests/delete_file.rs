use twin_core::NodeId;
use twin_emulate::{
    emulate_delete_file, DeleteFileInput, EmulationConfiguredService, EmulationEvidenceLine,
    EmulationNode, OverlayNodeState, DELETE_SAFETY_STATEMENT, SAFETY_STATEMENT,
};

fn nginx_file() -> EmulationNode {
    EmulationNode {
        id: NodeId::file("/etc/nginx/nginx.conf"),
        label: "/etc/nginx/nginx.conf".to_string(),
    }
}

fn nginx_service() -> EmulationConfiguredService {
    EmulationConfiguredService {
        id: NodeId::service("nginx.service"),
        label: "nginx.service".to_string(),
        file_id: NodeId::file("/etc/nginx/nginx.conf"),
        evidence: vec![EmulationEvidenceLine {
            source: "config_file_discovery".to_string(),
            statement: "known nginx config path".to_string(),
            relationship: "configured_by".to_string(),
            strength_score: 70,
        }],
    }
}

#[test]
fn delete_file_overlay_marks_only_file_hypothetically_deleted() {
    let input = DeleteFileInput {
        target: nginx_file(),
        configured_services: vec![nginx_service()],
        evidence: vec![],
        unknowns: vec![],
        file_in_graph: true,
        file_exists: true,
    };
    let report = emulate_delete_file(input);
    assert_eq!(report.overlay.node_overrides.len(), 1);
    assert_eq!(
        report.overlay.node_overrides[0].state,
        OverlayNodeState::HypotheticallyDeleted
    );
    assert!(report.overlay.interrupted_relationships.is_empty());
}

#[test]
fn delete_file_classifies_configured_service_as_low_runtime_critical_restart() {
    let report = emulate_delete_file(DeleteFileInput {
        target: nginx_file(),
        configured_services: vec![nginx_service()],
        evidence: vec![],
        unknowns: vec![],
        file_in_graph: true,
        file_exists: true,
    });
    assert_eq!(report.runtime_impacts.len(), 1);
    assert!(report.runtime_impacts[0]
        .statement
        .contains("Low immediate impact"));
    assert_eq!(report.restart_impacts.len(), 1);
    assert!(report.restart_impacts[0]
        .statement
        .contains("May fail to reload or restart"));
    assert_eq!(report.risk_level, twin_core::RiskLevel::Critical);
}

#[test]
fn delete_file_empty_configured_services_returns_unknown_impact() {
    let report = emulate_delete_file(DeleteFileInput {
        target: nginx_file(),
        configured_services: vec![],
        evidence: vec![],
        unknowns: vec![],
        file_in_graph: true,
        file_exists: true,
    });
    assert_eq!(report.unknown_impacts.len(), 1);
    assert_eq!(report.risk_level, twin_core::RiskLevel::Low);
}

#[test]
fn delete_file_scoring_keeps_risk_and_evidence_separate() {
    let report = emulate_delete_file(DeleteFileInput {
        target: nginx_file(),
        configured_services: vec![nginx_service()],
        evidence: vec![],
        unknowns: vec![],
        file_in_graph: true,
        file_exists: true,
    });
    assert_eq!(report.safety_statement, DELETE_SAFETY_STATEMENT);
    assert_eq!(report.general_safety_statement, SAFETY_STATEMENT);
    assert_ne!(
        report.risk_level.to_string(),
        report.evidence_strength.label().to_string()
    );
}
