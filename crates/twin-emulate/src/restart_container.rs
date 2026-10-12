use twin_core::{EdgeId, EdgeKind, EvidenceStrength, NodeId, NodeState, RiskLevel};

use crate::action::{
    EmulationAction, CONTAINER_RESTART_SAFETY_STATEMENT, RESTART_ACTION, SAFETY_STATEMENT,
};
use crate::effective_view::EffectiveGraphView;
use crate::input::{EmulationImpactPath, EmulationNode};
use crate::overlay::{GraphOverlay, InterruptedRelationship, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayInterrupted, EmulationOverlayNode,
    EmulationOverlaySummary,
};

#[derive(Debug, Clone)]
pub struct ContainerAffectedService {
    pub id: NodeId,
    pub label: String,
    pub edge_id: EdgeId,
    pub kind: EdgeKind,
    pub port: NodeId,
}

#[derive(Debug, Clone)]
pub struct RestartContainerInput {
    pub container: EmulationNode,
    pub container_in_graph: bool,
    pub affected: Vec<ContainerAffectedService>,
    pub impact_paths: Vec<EmulationImpactPath>,
}

pub struct RestartContainerOverlayBuilder {
    container: NodeId,
    label: String,
    interrupted: Vec<InterruptedRelationship>,
    node_overrides: Vec<NodeOverlay>,
}

impl RestartContainerOverlayBuilder {
    pub fn new(
        container: NodeId,
        label: impl Into<String>,
        affected: &[ContainerAffectedService],
        paths: &[EmulationImpactPath],
    ) -> Self {
        let label = label.into();
        let mut interrupted: Vec<InterruptedRelationship> = affected
            .iter()
            .map(|service| InterruptedRelationship {
                edge_id: service.edge_id.clone(),
                from: service.id.clone(),
                to: service.port.clone(),
                kind: service.kind,
                reason: format!(
                    "{} would lose {} during a hypothetical restart",
                    service.label, service.port
                ),
            })
            .collect();
        let mut node_overrides = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for service in affected {
            if seen.insert(service.id.clone()) {
                node_overrides.push(NodeOverlay {
                    node_id: service.id.clone(),
                    state: OverlayNodeState::TemporarilyUnavailable,
                    reason: format!(
                        "{} may be unavailable while its container dependency restarts",
                        service.label
                    ),
                });
            }
        }
        for path in paths {
            for step in &path.steps {
                if seen.insert(step.from_id.clone()) {
                    node_overrides.push(NodeOverlay {
                        node_id: step.from_id.clone(),
                        state: OverlayNodeState::TemporarilyUnavailable,
                        reason: format!(
                            "{} may be unavailable through its runtime dependency chain",
                            step.from_label
                        ),
                    });
                }
                if !interrupted.iter().any(|edge| edge.edge_id == step.edge_id) {
                    interrupted.push(InterruptedRelationship {
                        edge_id: step.edge_id.clone(),
                        from: step.from_id.clone(),
                        to: step.to_id.clone(),
                        kind: step.edge_kind,
                        reason: "runtime dependency may be interrupted during container restart"
                            .into(),
                    });
                }
            }
        }
        Self {
            container,
            label,
            interrupted,
            node_overrides,
        }
    }

    pub fn build(mut self) -> GraphOverlay {
        self.node_overrides.push(NodeOverlay {
            node_id: self.container.clone(),
            state: OverlayNodeState::TemporarilyUnavailable,
            reason: format!(
                "{} would be unavailable during a hypothetical restart",
                self.label
            ),
        });
        self.node_overrides
            .sort_by(|a, b| a.node_id.as_str().cmp(b.node_id.as_str()));
        self.interrupted
            .sort_by(|a, b| a.edge_id.as_str().cmp(b.edge_id.as_str()));
        GraphOverlay {
            action: EmulationAction::RestartContainer {
                target: self.container.clone(),
            },
            node_overrides: self.node_overrides,
            interrupted_relationships: self.interrupted,
        }
    }
}

pub fn emulate_restart_container(input: RestartContainerInput) -> EmulationDomainReport {
    let affected = if input.container_in_graph {
        input.affected
    } else {
        Vec::new()
    };
    let overlay = RestartContainerOverlayBuilder::new(
        input.container.id.clone(),
        input.container.label.clone(),
        &affected,
        &input.impact_paths,
    )
    .build();
    let base_nodes = std::slice::from_ref(&input.container);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);
    let overlay_summary = EmulationOverlaySummary {
        unavailable_nodes: view
            .overlay()
            .node_overrides
            .iter()
            .map(|node| EmulationOverlayNode {
                id: node.node_id.to_string(),
                label: if node.node_id == input.container.id {
                    input.container.label.clone()
                } else {
                    node.node_id.to_string()
                },
                reason: node.reason.clone(),
            })
            .collect(),
        interrupted_relationships: view
            .overlay()
            .interrupted_relationships
            .iter()
            .map(|edge| EmulationOverlayInterrupted {
                edge_id: edge.edge_id.to_string(),
                from: edge.from.to_string(),
                to: edge.to.to_string(),
                kind: edge.kind.to_string(),
                reason: edge.reason.clone(),
            })
            .collect(),
    };
    let impacts: Vec<EmulationImpact> = affected
        .iter()
        .map(|service| impact(service, &input.container.label))
        .collect();
    let (risk_level, risk_reasons) =
        score(&input.container.label, &impacts, input.container_in_graph);
    let evidence = if impacts.iter().any(|item| item.path.contains("connects_to")) {
        EvidenceStrength::strong()
    } else if impacts.is_empty() {
        EvidenceStrength::weak()
    } else {
        EvidenceStrength::moderate()
    };
    EmulationDomainReport {
        action: RESTART_ACTION.to_string(),
        target: input.container.id.to_string(),
        target_label: input.container.label,
        action_performed: false,
        safety_statement: CONTAINER_RESTART_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level,
        risk_reasons,
        evidence_strength: evidence,
        evidence_reasons: vec!["published port relationships in the current graph".to_string()],
        overlay,
        overlay_summary,
        transient_impacts: impacts,
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

fn impact(service: &ContainerAffectedService, container_label: &str) -> EmulationImpact {
    let verb = match service.kind {
        EdgeKind::DependsOn => "depends on",
        _ => "connects to",
    };
    EmulationImpact {
        id: service.id.to_string(),
        label: service.label.clone(),
        statement: format!("{} {verb} {container_label} port", service.id.as_str()),
        path: format!(
            "{} {} {}",
            service.id.as_str(),
            service.kind,
            service.port.as_str()
        ),
        evidence: vec![format!("graph edge {}", service.edge_id.as_str())],
    }
}

fn score(label: &str, impacts: &[EmulationImpact], in_graph: bool) -> (RiskLevel, Vec<String>) {
    if !in_graph {
        return (
            RiskLevel::Low,
            vec![format!(
                "{label} is not in the graph; run `twin scan` first"
            )],
        );
    }
    if impacts.is_empty() {
        return (
            RiskLevel::Low,
            vec!["nothing connects to a published port".to_string()],
        );
    }
    let mut reasons = vec![format!(
        "{} service(s) connect to a published port of {label}",
        impacts.len()
    )];
    for impact in impacts {
        reasons.push(impact.statement.clone());
    }
    (RiskLevel::High, reasons)
}
