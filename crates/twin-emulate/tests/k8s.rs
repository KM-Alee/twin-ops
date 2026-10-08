use twin_core::{NodeId, RiskLevel};
use twin_emulate::{
    emulate_delete_k8s_pod, emulate_rollout_k8s_deployment, pod_delete_risk, DeleteK8sPodInput,
    EmulationNode, ReadyService, RolloutK8sDeploymentInput, LAST_READY_ENDPOINT_REASON,
};

fn pod(name: &str) -> EmulationNode {
    EmulationNode {
        id: NodeId::k8s_pod("default", name),
        label: name.to_string(),
    }
}

#[test]
fn deleting_one_ready_pod_stays_low_when_another_remains() {
    let (level, reason) = pod_delete_risk(
        "api-123",
        &[ReadyService {
            id: NodeId::k8s_service("default", "api"),
            label: "api".to_string(),
            ready_pods: vec!["api-123".to_string(), "api-124".to_string()],
        }],
    );
    assert_eq!(level, RiskLevel::Low);
    assert_eq!(reason, "service still has 1 ready endpoint.");
    let report = emulate_delete_k8s_pod(DeleteK8sPodInput {
        pod: pod("api-123"),
        pod_name: "api-123".to_string(),
        pod_in_graph: true,
        services: vec![ReadyService {
            id: NodeId::k8s_service("default", "api"),
            label: "api".to_string(),
            ready_pods: vec!["api-123".to_string(), "api-124".to_string()],
        }],
    });
    assert!(!report.action_performed);
    assert_eq!(report.safety_statement, "No pod was deleted.");
    assert_eq!(report.risk_level, RiskLevel::Low);
}

#[test]
fn deleting_the_last_ready_pod_is_high() {
    let (level, reason) = pod_delete_risk(
        "api-123",
        &[ReadyService {
            id: NodeId::k8s_service("default", "api"),
            label: "api".to_string(),
            ready_pods: vec!["api-123".to_string()],
        }],
    );
    assert_eq!(level, RiskLevel::High);
    assert_eq!(reason, LAST_READY_ENDPOINT_REASON);
}

#[test]
fn rollout_lists_owned_pods_and_performs_nothing() {
    let report = emulate_rollout_k8s_deployment(RolloutK8sDeploymentInput {
        deployment: EmulationNode {
            id: NodeId::k8s_deployment("default", "api"),
            label: "api".to_string(),
        },
        deployment_in_graph: true,
        pods: vec![pod("api-123"), pod("api-124")],
    });
    assert!(!report.action_performed);
    assert_eq!(report.action, "rollout");
    assert_eq!(report.safety_statement, "No deployment was rolled out.");
    assert_eq!(report.restart_impacts.len(), 2);
    assert!(report
        .restart_impacts
        .iter()
        .any(|impact| impact.id == "k8s:pod:default/api-123"));
}
