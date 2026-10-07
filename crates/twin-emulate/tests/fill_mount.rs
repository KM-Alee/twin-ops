use twin_core::{NodeId, RiskLevel};
use twin_emulate::{
    emulate_fill_mount, fill_risk, EmulationNode, FillAffectedService, FillMountInput,
    FillMountOverlayBuilder, OverlayNodeState, FILL_DISK_SAFETY_STATEMENT,
};

#[test]
fn fill_overlay_is_hypothetical() {
    let mount = NodeId::mount("/var");
    let overlay = FillMountOverlayBuilder::new(mount, "/var", 95).build();
    assert!(overlay
        .node_overrides
        .iter()
        .all(|node| node.state == OverlayNodeState::HypotheticallyFilled));
    assert!(overlay.interrupted_relationships.is_empty());
}

#[test]
fn fill_to_95_is_critical_and_performs_nothing() {
    let report = emulate_fill_mount(FillMountInput {
        mount: EmulationNode {
            id: NodeId::mount("/var"),
            label: "/var".to_string(),
        },
        to_percent: 95,
        current_used_percent: Some(40),
        mount_in_graph: true,
        affected: vec![
            FillAffectedService {
                id: NodeId::service("postgresql.service"),
                label: "postgresql.service".to_string(),
                path: "/var/lib/postgresql".to_string(),
                logs: false,
            },
            FillAffectedService {
                id: NodeId::service("systemd-journald.service"),
                label: "systemd-journald.service".to_string(),
                path: "/var/log/journal".to_string(),
                logs: true,
            },
        ],
        unknowns: Vec::new(),
    });
    assert!(!report.action_performed);
    assert_eq!(report.action, "fill-disk");
    assert_eq!(report.risk_level, RiskLevel::Critical);
    assert_eq!(report.safety_statement, FILL_DISK_SAFETY_STATEMENT);
    assert!(report
        .persistent_impacts
        .iter()
        .any(|impact| impact.statement == "postgresql.service uses /var/lib/postgresql"));
    assert!(report
        .persistent_impacts
        .iter()
        .any(|impact| impact.statement == "systemd-journald.service writes to /var/log/journal"));
    assert!(report
        .risk_reasons
        .iter()
        .any(|reason| reason.contains("currently 40%")));
}

#[test]
fn fill_risk_follows_the_percent() {
    assert_eq!(fill_risk(80), RiskLevel::Critical);
    assert_eq!(fill_risk(95), RiskLevel::Critical);
    assert_eq!(fill_risk(79), RiskLevel::High);
    assert_eq!(fill_risk(40), RiskLevel::Medium);
}
