use twin_core::{EvidenceRecency, NodeId, RiskLevel};
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
        coverage_gaps: Vec::new(),
        recency: EvidenceRecency::Fresh,
        source: "fixture".into(),
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
        policy: None,
        services: Vec::new(),
        coverage_gaps: Vec::new(),
        recency: EvidenceRecency::Fresh,
        source: "fixture".into(),
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

fn rollout_input(is_recreate: bool, unavailable: u32) -> RolloutK8sDeploymentInput {
    RolloutK8sDeploymentInput {
        deployment: EmulationNode {
            id: NodeId::k8s_deployment("default", "api"),
            label: "api".into(),
        },
        deployment_in_graph: true,
        pods: vec![pod("api-123"), pod("api-124")],
        policy: Some(twin_emulate::RolloutPolicy {
            desired_replicas: 2,
            is_recreate,
            max_unavailable: unavailable,
            max_surge: 1,
        }),
        services: vec![ReadyService {
            id: NodeId::k8s_service("default", "api"),
            label: "api".into(),
            ready_pods: vec!["api-123".into(), "api-124".into()],
        }],
        coverage_gaps: Vec::new(),
        recency: EvidenceRecency::Fresh,
        source: "fixture-read:1".into(),
    }
}

#[test]
fn rollout_recreate_can_remove_every_ready_endpoint() {
    let report = emulate_rollout_k8s_deployment(rollout_input(true, 2));
    assert_eq!(report.risk_level, RiskLevel::High);
    assert_eq!(report.overlay_summary.unavailable_nodes.len(), 2);
    assert!(report.transient_impacts[0].statement.contains("at least 0"));
}

#[test]
fn rolling_update_uses_the_budget_without_deleting_all_pods_at_once() {
    let report = emulate_rollout_k8s_deployment(rollout_input(false, 1));
    assert_eq!(report.risk_level, RiskLevel::Medium);
    assert!(report.overlay_summary.unavailable_nodes.is_empty());
    assert!(report.transient_impacts[0].statement.contains("at least 1"));
    let protected = emulate_rollout_k8s_deployment(rollout_input(false, 0));
    assert_eq!(protected.risk_level, RiskLevel::Low);
    assert!(protected
        .risk_reasons
        .iter()
        .any(|reason| reason.contains("scheduling")));
}

#[test]
fn rollout_cannot_claim_availability_without_current_complete_evidence() {
    for gap in ["endpoint read denied", "cluster read interrupted"] {
        let mut input = rollout_input(false, 0);
        input.coverage_gaps.push(gap.into());
        let report = emulate_rollout_k8s_deployment(input);
        assert_eq!(report.risk_level, RiskLevel::Unknown);
        assert!(report.evidence_strength.score() < 31);
    }
    let mut input = rollout_input(false, 0);
    input.recency = EvidenceRecency::Stale;
    assert_eq!(
        emulate_rollout_k8s_deployment(input).risk_level,
        RiskLevel::Unknown
    );
    let mut input = rollout_input(false, 0);
    input.policy = None;
    assert_eq!(
        emulate_rollout_k8s_deployment(input).risk_level,
        RiskLevel::Unknown
    );
}

#[test]
fn readiness_from_another_workload_survives_recreate() {
    let mut input = rollout_input(true, 2);
    input.services[0]
        .ready_pods
        .push("other-deployment-pod".into());
    assert_eq!(
        emulate_rollout_k8s_deployment(input).risk_level,
        RiskLevel::Medium
    );
}

#[test]
fn pod_delete_with_unknown_readiness_does_not_claim_low_risk() {
    let report = emulate_delete_k8s_pod(DeleteK8sPodInput {
        pod: pod("api-123"),
        pod_name: "api-123".into(),
        pod_in_graph: true,
        services: Vec::new(),
        coverage_gaps: vec!["endpoint read denied".into()],
        recency: EvidenceRecency::Fresh,
        source: "fixture-read:1".into(),
    });
    assert_eq!(report.risk_level, RiskLevel::Unknown);
    assert!(report.evidence_strength.score() < 31);
    assert!(!report.unknown_impacts.is_empty());
}

#[test]
fn pod_delete_counts_unique_endpoints_and_exposes_existing_unavailability() {
    let service = |ready_pods| ReadyService {
        id: NodeId::k8s_service("default", "api"),
        label: "api".into(),
        ready_pods,
    };
    assert_eq!(
        pod_delete_risk(
            "api-123",
            &[service(vec!["api-123".into(), "api-123".into()])]
        )
        .0,
        RiskLevel::High
    );
    let (risk, reason) = pod_delete_risk("api-123", &[service(Vec::new())]);
    assert_eq!(risk, RiskLevel::High);
    assert!(reason.contains("already has no ready endpoints"));
}
