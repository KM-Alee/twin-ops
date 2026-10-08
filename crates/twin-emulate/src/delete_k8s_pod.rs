use twin_core::{EvidenceStrength, NodeId, NodeState, RiskLevel};

use crate::action::{
    EmulationAction, DELETE_ACTION, K8S_POD_DELETE_SAFETY_STATEMENT, SAFETY_STATEMENT,
};
use crate::effective_view::EffectiveGraphView;
use crate::input::EmulationNode;
use crate::overlay::{GraphOverlay, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
};

pub const LAST_READY_ENDPOINT_REASON: &str = "service would lose its last ready endpoint.";

#[derive(Debug, Clone)]
pub struct ReadyService {
    pub id: NodeId,
    pub label: String,
    pub ready_pods: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct DeleteK8sPodInput {
    pub pod: EmulationNode,
    pub pod_name: String,
    pub pod_in_graph: bool,
    pub services: Vec<ReadyService>,
}

pub struct DeleteK8sPodOverlayBuilder {
    pod: NodeId,
    label: String,
}

impl DeleteK8sPodOverlayBuilder {
    pub fn new(pod: NodeId, label: impl Into<String>) -> Self {
        Self {
            pod,
            label: label.into(),
        }
    }

    pub fn build(self) -> GraphOverlay {
        GraphOverlay {
            action: EmulationAction::DeleteK8sPod {
                target: self.pod.clone(),
            },
            node_overrides: vec![NodeOverlay {
                node_id: self.pod,
                state: OverlayNodeState::HypotheticallyDeleted,
                reason: format!(
                    "{} would be unavailable after a hypothetical removal",
                    self.label
                ),
            }],
            interrupted_relationships: Vec::new(),
        }
    }
}

pub fn ready_endpoint_reason(remaining: usize) -> String {
    if remaining == 1 {
        "service still has 1 ready endpoint.".to_string()
    } else {
        format!("service still has {remaining} ready endpoints.")
    }
}

pub fn pod_delete_risk(pod_name: &str, services: &[ReadyService]) -> (RiskLevel, String) {
    if services.is_empty() {
        return (RiskLevel::Low, "no service selects this pod.".to_string());
    }
    let mut min_remaining = usize::MAX;
    let mut drops_last = false;
    for service in services {
        let was_ready = service.ready_pods.iter().any(|name| name == pod_name);
        let remaining = service
            .ready_pods
            .iter()
            .filter(|name| name.as_str() != pod_name)
            .count();
        if was_ready && remaining == 0 {
            drops_last = true;
        }
        min_remaining = min_remaining.min(remaining);
    }
    if drops_last {
        return (RiskLevel::High, LAST_READY_ENDPOINT_REASON.to_string());
    }
    (RiskLevel::Low, ready_endpoint_reason(min_remaining))
}

pub fn emulate_delete_k8s_pod(input: DeleteK8sPodInput) -> EmulationDomainReport {
    let services = if input.pod_in_graph {
        input.services
    } else {
        Vec::new()
    };
    let (risk_level, reason) = if input.pod_in_graph {
        pod_delete_risk(&input.pod_name, &services)
    } else {
        (
            RiskLevel::Low,
            "pod is not in the graph; run `twin k8s scan` first".to_string(),
        )
    };
    let overlay =
        DeleteK8sPodOverlayBuilder::new(input.pod.id.clone(), input.pod.label.clone()).build();
    let base_nodes = std::slice::from_ref(&input.pod);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);
    let overlay_summary = EmulationOverlaySummary {
        unavailable_nodes: view
            .overlay()
            .node_overrides
            .iter()
            .map(|node| EmulationOverlayNode {
                id: node.node_id.to_string(),
                label: input.pod.label.clone(),
                reason: node.reason.clone(),
            })
            .collect(),
        interrupted_relationships: Vec::new(),
    };
    let transient_impacts = services
        .iter()
        .map(|service| EmulationImpact {
            id: service.id.to_string(),
            label: service.label.clone(),
            statement: reason.clone(),
            path: format!("{} selects {}", service.id.as_str(), input.pod.id.as_str()),
            evidence: vec!["service selector and ready endpoints".to_string()],
        })
        .collect();
    EmulationDomainReport {
        action: DELETE_ACTION.to_string(),
        target: input.pod.id.to_string(),
        target_label: input
            .pod
            .id
            .k8s_short()
            .unwrap_or(input.pod.label.as_str())
            .to_string(),
        action_performed: false,
        safety_statement: K8S_POD_DELETE_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level,
        risk_reasons: vec![reason],
        evidence_strength: EvidenceStrength::moderate(),
        evidence_reasons: vec!["kubernetes ownership and selector edges".to_string()],
        overlay,
        overlay_summary,
        transient_impacts,
        configured_impacts: Vec::new(),
        runtime_impacts: Vec::new(),
        restart_impacts: Vec::new(),
        persistent_impacts: Vec::new(),
        unknown_impacts: Vec::new(),
        evidence_lines: Vec::new(),
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 1,
    }
}
