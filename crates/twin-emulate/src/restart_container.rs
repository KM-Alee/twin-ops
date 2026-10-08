use twin_core::{EdgeId, EdgeKind, EvidenceStrength, NodeId, NodeState, RiskLevel};

use crate::action::{
    EmulationAction, CONTAINER_RESTART_SAFETY_STATEMENT, RESTART_ACTION, SAFETY_STATEMENT,
};
use crate::effective_view::EffectiveGraphView;
use crate::input::EmulationNode;
use crate::overlay::{GraphOverlay, InterruptedRelationship, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
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
}

pub struct RestartContainerOverlayBuilder {
    container: NodeId,
    label: String,
    interrupted: Vec<InterruptedRelationship>,
}

impl RestartContainerOverlayBuilder {
    pub fn new(
        container: NodeId,
        label: impl Into<String>,
        affected: &[ContainerAffectedService],
    ) -> Self {
        let label = label.into();
        let interrupted = affected
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
        Self {
            container,
            label,
            interrupted,
        }
    }

    pub fn build(self) -> GraphOverlay {
        GraphOverlay {
            action: EmulationAction::RestartContainer {
                target: self.container.clone(),
            },
            node_overrides: vec![NodeOverlay {
                node_id: self.container,
                state: OverlayNodeState::TemporarilyUnavailable,
                reason: format!(
                    "{} would be unavailable during a hypothetical restart",
                    self.label
                ),
            }],
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
                label: input.container.label.clone(),
                reason: node.reason.clone(),
            })
            .collect(),
        interrupted_relationships: Vec::new(),
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
