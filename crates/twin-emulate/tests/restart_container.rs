use twin_core::{EdgeId, EdgeKind, NodeId, RiskLevel};
use twin_emulate::{
    emulate_restart_container, ContainerAffectedService, EmulationNode, RestartContainerInput,
    RestartContainerOverlayBuilder,
};

fn redis() -> EmulationNode {
    EmulationNode {
        id: NodeId::container("redis"),
        label: "redis".to_string(),
    }
}

#[test]
fn connected_service_is_high_risk_and_performs_nothing() {
    let port = NodeId::port_tcp("0.0.0.0", 6379).expect("port");
    let service = NodeId::service("api.service");
    let report = emulate_restart_container(RestartContainerInput {
        container: redis(),
        container_in_graph: true,
        affected: vec![ContainerAffectedService {
            id: service.clone(),
            label: "api.service".to_string(),
            edge_id: EdgeId::new(&service, EdgeKind::ConnectsTo, &port),
            kind: EdgeKind::ConnectsTo,
            port,
        }],
    });
    assert!(!report.action_performed);
    assert_eq!(report.risk_level, RiskLevel::High);
    assert_eq!(report.safety_statement, "No container was restarted.");
    assert!(report
        .transient_impacts
        .iter()
        .any(|impact| impact.statement == "service:api.service connects to redis port"));
    assert!(report
        .overlay
        .node_overrides
        .iter()
        .any(|node| node.node_id.as_str() == "container:redis"));
}

#[test]
fn isolated_container_is_lower_risk_and_performs_nothing() {
    let report = emulate_restart_container(RestartContainerInput {
        container: redis(),
        container_in_graph: true,
        affected: Vec::new(),
    });
    assert!(!report.action_performed);
    assert_eq!(report.risk_level, RiskLevel::Low);
    assert!(report
        .risk_reasons
        .iter()
        .any(|reason| reason.contains("nothing connects")));
    let overlay =
        RestartContainerOverlayBuilder::new(NodeId::container("redis"), "redis", &[]).build();
    assert!(overlay.interrupted_relationships.is_empty());
}
