use twin_core::{NodeId, RiskLevel};
use twin_emulate::{
    emulate_upgrade_package, EmulationNode, OverlayNodeState, UpgradeAffectedService,
    UpgradePackageInput, UpgradePackageOverlayBuilder, UPGRADE_SAFETY_STATEMENT,
};

#[test]
fn upgrade_overlay_is_hypothetical() {
    let package = NodeId::package("openssl");
    let overlay = UpgradePackageOverlayBuilder::new(package, "openssl").build();
    assert!(overlay
        .node_overrides
        .iter()
        .all(|node| node.state == OverlayNodeState::HypotheticallyUpgraded));
    assert!(overlay.interrupted_relationships.is_empty());
}

#[test]
fn upgrade_reports_low_runtime_and_high_restart() {
    let report = emulate_upgrade_package(UpgradePackageInput {
        package: EmulationNode {
            id: NodeId::package("openssl"),
            label: "openssl".to_string(),
        },
        package_in_graph: true,
        mapped_stems: vec!["libssl".to_string()],
        affected: vec![UpgradeAffectedService {
            id: NodeId::service("nginx.service"),
            label: "nginx.service".to_string(),
        }],
        unknowns: Vec::new(),
    });
    assert!(!report.action_performed);
    assert_eq!(report.action, "upgrade");
    assert_eq!(report.safety_statement, UPGRADE_SAFETY_STATEMENT);
    assert_eq!(report.risk_level, RiskLevel::High);
    assert_eq!(report.runtime_impacts[0].label, "LOW");
    assert!(report.runtime_impacts[0]
        .statement
        .contains("running processes already have current libssl mapped."));
    assert!(report
        .restart_impacts
        .iter()
        .any(|impact| impact.label == "nginx.service"));
}

#[test]
fn missing_package_does_not_invent_services() {
    let report = emulate_upgrade_package(UpgradePackageInput {
        package: EmulationNode {
            id: NodeId::package("openssl"),
            label: "openssl".to_string(),
        },
        package_in_graph: false,
        mapped_stems: vec!["libssl".to_string()],
        affected: vec![UpgradeAffectedService {
            id: NodeId::service("nginx.service"),
            label: "nginx.service".to_string(),
        }],
        unknowns: Vec::new(),
    });
    assert!(!report.action_performed);
    assert_eq!(report.risk_level, RiskLevel::Unknown);
    assert!(report.restart_impacts.is_empty());
    assert_eq!(report.runtime_impacts[0].label, "UNKNOWN");
    assert!(report
        .unknown_impacts
        .iter()
        .any(|impact| impact.statement.contains("not in the graph")));
}
