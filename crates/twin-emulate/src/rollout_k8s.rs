use twin_core::{
    score_evidence, EvidenceFactors, EvidenceRecency, EvidenceSourceKind, NodeState, RiskLevel,
};

use crate::action::{
    EmulationAction, K8S_ROLLOUT_SAFETY_STATEMENT, ROLLOUT_ACTION, SAFETY_STATEMENT,
};
use crate::delete_k8s_pod::ReadyService;
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
    pub policy: Option<RolloutPolicy>,
    pub services: Vec<ReadyService>,
    pub coverage_gaps: Vec<String>,
    pub recency: EvidenceRecency,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct RolloutPolicy {
    pub desired_replicas: u32,
    pub is_recreate: bool,
    pub max_unavailable: u32,
    pub max_surge: u32,
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
    let (risk_level, mut risk_reasons, transient_impacts) = assess_availability(&input);
    let pods = if input.deployment_in_graph {
        input.pods.clone()
    } else {
        Vec::new()
    };
    let mut gaps = input.coverage_gaps.clone();
    if input.policy.is_none() {
        gaps.push("deployment rollout policy is unknown".into());
    }
    if !input.deployment_in_graph {
        gaps.push("deployment is not in the graph".into());
    }
    if matches!(
        input.recency,
        EvidenceRecency::Unknown | EvidenceRecency::Stale
    ) {
        gaps.push("Kubernetes observations are missing or stale".into());
    }
    risk_reasons.extend(gaps.iter().cloned());
    risk_reasons.push("new pod scheduling, readiness, application compatibility and connection draining are unknown".into());
    let evidence = score_evidence(EvidenceFactors {
        source: if gaps.is_empty() {
            EvidenceSourceKind::Config
        } else {
            EvidenceSourceKind::None
        },
        recency: input.recency,
        independent_sources: 1,
        ..EvidenceFactors::none()
    });
    // A rolling policy identifies a budget, not which pods are unavailable together.
    let unavailable_pods = if input
        .policy
        .as_ref()
        .is_some_and(|policy| policy.is_recreate)
    {
        pods.clone()
    } else {
        Vec::new()
    };
    let overlay = RolloutK8sDeploymentOverlayBuilder::new(
        input.deployment.id.clone(),
        input.deployment.label.clone(),
        unavailable_pods,
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
                "{} would be replaced during the rollout",
                pod.id.k8s_short().unwrap_or(pod.id.as_str())
            ),
            path: format!("{} owns {}", input.deployment.id.as_str(), pod.id.as_str()),
            evidence: vec![format!("Observed ownership from {}", input.source)],
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
        risk_level,
        risk_reasons,
        evidence_strength: evidence.strength,
        evidence_reasons: evidence.reasons,
        overlay,
        overlay_summary,
        transient_impacts,
        configured_impacts: Vec::new(),
        runtime_impacts: Vec::new(),
        restart_impacts,
        persistent_impacts: Vec::new(),
        unknown_impacts: gaps
            .iter()
            .map(|gap| EmulationImpact {
                id: input.deployment.id.to_string(),
                label: "unknown".into(),
                statement: gap.clone(),
                path: String::new(),
                evidence: Vec::new(),
            })
            .collect(),
        evidence_lines: vec![format!(
            "Observed deployment policy, ownership and service readiness from {}",
            input.source
        )],
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 1,
    }
}

fn assess_availability(
    input: &RolloutK8sDeploymentInput,
) -> (RiskLevel, Vec<String>, Vec<EmulationImpact>) {
    let Some(policy) = &input.policy else {
        return (
            RiskLevel::Unknown,
            vec!["availability cannot be estimated without a rollout policy".into()],
            Vec::new(),
        );
    };
    if !input.deployment_in_graph
        || !input.coverage_gaps.is_empty()
        || matches!(
            input.recency,
            EvidenceRecency::Unknown | EvidenceRecency::Stale
        )
    {
        return (
            RiskLevel::Unknown,
            vec![
                "availability cannot be estimated from incomplete or stale Kubernetes evidence"
                    .into(),
            ],
            Vec::new(),
        );
    }
    if policy.desired_replicas > 0 && input.pods.is_empty() {
        return (
            RiskLevel::Unknown,
            vec!["deployment desires pods but no owned pods were observed".into()],
            Vec::new(),
        );
    }
    let mut reasons = vec![if policy.is_recreate {
        "Recreate replaces all owned pods before replacement availability is established".into()
    } else {
        format!("RollingUpdate permits {} unavailable pod(s) and {} surge pod(s) for {} desired replicas", policy.max_unavailable, policy.max_surge, policy.desired_replicas)
    }];
    let names: std::collections::HashSet<_> = input
        .pods
        .iter()
        .filter_map(|pod| pod.id.k8s_namespace_and_name().map(|(_, name)| name))
        .collect();
    let mut impacts = Vec::new();
    let mut level = if policy.desired_replicas == 0 && input.pods.is_empty() {
        RiskLevel::Low
    } else {
        RiskLevel::Unknown
    };
    for service in &input.services {
        let ready: std::collections::HashSet<_> = service.ready_pods.iter().collect();
        let owned_ready = ready
            .iter()
            .filter(|name| names.contains(name.as_str()))
            .count();
        let removed = if policy.is_recreate {
            owned_ready
        } else {
            owned_ready.min(policy.max_unavailable as usize)
        };
        let remaining = ready.len().saturating_sub(removed);
        let service_level = if remaining == 0 {
            RiskLevel::High
        } else if removed > 0 {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        };
        level = match (level, service_level) {
            (RiskLevel::High, _) | (_, RiskLevel::High) => RiskLevel::High,
            (RiskLevel::Medium, _) | (_, RiskLevel::Medium) => RiskLevel::Medium,
            _ => RiskLevel::Low,
        };
        let statement = format!("Inferred: {} has {} observed ready endpoint(s); at least {remaining} current endpoint(s) remain under the rollout budget, assuming unchanged readiness and compliant controller behavior", service.label, ready.len());
        reasons.push(statement.clone());
        impacts.push(EmulationImpact {
            id: service.id.to_string(),
            label: service.label.clone(),
            statement,
            path: format!(
                "{} selects owned pods of {}",
                service.id, input.deployment.id
            ),
            evidence: vec![format!(
                "Observed ready endpoints and ownership from {}",
                input.source
            )],
        });
    }
    if input.services.is_empty() && policy.desired_replicas > 0 {
        reasons.push("no selecting service was observed; non-Service consumers are unknown".into());
    }
    (level, reasons, impacts)
}
