use twin_core::{EvidenceStrength, NodeState, RiskLevel};

use crate::action::{
    EmulationAction, K8S_ROLLOUT_SAFETY_STATEMENT, ROLLOUT_ACTION, SAFETY_STATEMENT,
};
use crate::effective_view::EffectiveGraphView;
use crate::input::EmulationNode;
use crate::overlay::{GraphOverlay, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
};

#[derive(Debug, Clone)]
pub struct RolloutK8sDeploymentInput {
    pub deployment: EmulationNode,
    pub deployment_in_graph: bool,
    pub pods: Vec<EmulationNode>,
}

pub struct RolloutK8sDeploymentOverlayBuilder {
    deployment: twin_core::NodeId,
    label: String,
    pods: Vec<EmulationNode>,
}

impl RolloutK8sDeploymentOverlayBuilder {
    pub fn new(
        deployment: twin_core::NodeId,
        label: impl Into<String>,
        pods: Vec<EmulationNode>,
    ) -> Self {
        Self {
            deployment,
            label: label.into(),
            pods,
        }
    }

    pub fn build(self) -> GraphOverlay {
        let node_overrides = self
            .pods
            .iter()
            .map(|pod| NodeOverlay {
                node_id: pod.id.clone(),
                state: OverlayNodeState::TemporarilyUnavailable,
                reason: format!(
                    "{} would restart during a hypothetical rollout of {}",
                    pod.label, self.label
                ),
            })
            .collect();
        GraphOverlay {
            action: EmulationAction::RolloutK8sDeployment {
                target: self.deployment,
            },
            node_overrides,
            interrupted_relationships: Vec::new(),
        }
    }
}

pub fn emulate_rollout_k8s_deployment(input: RolloutK8sDeploymentInput) -> EmulationDomainReport {
    let pods = if input.deployment_in_graph {
        input.pods
    } else {
        Vec::new()
    };
    let reason = if !input.deployment_in_graph {
        "deployment is not in the graph; run `twin k8s scan` first".to_string()
    } else if pods.is_empty() {
        "deployment owns no pods.".to_string()
    } else {
        format!("hypothetical rollout restarts {} owned pods.", pods.len())
    };
    let overlay = RolloutK8sDeploymentOverlayBuilder::new(
        input.deployment.id.clone(),
        input.deployment.label.clone(),
        pods.clone(),
    )
    .build();
    let base_nodes = std::slice::from_ref(&input.deployment);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);
    let overlay_summary = EmulationOverlaySummary {
        unavailable_nodes: view
            .overlay()
            .node_overrides
            .iter()
            .map(|node| {
                let label = pods
                    .iter()
                    .find(|pod| pod.id == node.node_id)
                    .map(|pod| pod.label.clone())
                    .unwrap_or_else(|| node.node_id.to_string());
                EmulationOverlayNode {
                    id: node.node_id.to_string(),
                    label,
                    reason: node.reason.clone(),
                }
            })
            .collect(),
        interrupted_relationships: Vec::new(),
    };
    let restart_impacts = pods
        .iter()
        .map(|pod| EmulationImpact {
            id: pod.id.to_string(),
            label: pod.label.clone(),
            statement: format!(
                "{} would restart",
                pod.id.k8s_short().unwrap_or(pod.id.as_str())
            ),
            path: format!("{} owns {}", input.deployment.id.as_str(), pod.id.as_str()),
            evidence: vec!["replicaset ownership edges".to_string()],
        })
        .collect();
    EmulationDomainReport {
        action: ROLLOUT_ACTION.to_string(),
        target: input.deployment.id.to_string(),
        target_label: input
            .deployment
            .id
            .k8s_short()
            .unwrap_or(input.deployment.label.as_str())
            .to_string(),
        action_performed: false,
        safety_statement: K8S_ROLLOUT_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level: RiskLevel::Low,
        risk_reasons: vec![reason],
        evidence_strength: EvidenceStrength::moderate(),
        evidence_reasons: vec!["kubernetes ownership edges".to_string()],
        overlay,
        overlay_summary,
        transient_impacts: Vec::new(),
        configured_impacts: Vec::new(),
        runtime_impacts: Vec::new(),
        restart_impacts,
        persistent_impacts: Vec::new(),
        unknown_impacts: Vec::new(),
        evidence_lines: Vec::new(),
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 1,
    }
}
