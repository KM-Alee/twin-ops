use twin_core::{EvidenceStrength, NodeId, NodeState, RiskLevel};

use crate::action::{
    EmulationAction, FILL_DISK_ACTION, FILL_DISK_SAFETY_STATEMENT, SAFETY_STATEMENT,
};
use crate::effective_view::EffectiveGraphView;
use crate::input::{EmulationNode, EmulationUnknown};
use crate::overlay::{GraphOverlay, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
};

#[derive(Debug, Clone)]
pub struct FillAffectedService {
    pub id: NodeId,
    pub label: String,
    pub path: String,
    pub logs: bool,
}

#[derive(Debug, Clone)]
pub struct FillMountInput {
    pub mount: EmulationNode,
    pub to_percent: u8,
    pub current_used_percent: Option<u8>,
    pub mount_in_graph: bool,
    pub affected: Vec<FillAffectedService>,
    pub unknowns: Vec<EmulationUnknown>,
}

pub struct FillMountOverlayBuilder {
    mount: NodeId,
    label: String,
    to_percent: u8,
}

impl FillMountOverlayBuilder {
    pub fn new(mount: NodeId, label: impl Into<String>, to_percent: u8) -> Self {
        Self {
            mount,
            label: label.into(),
            to_percent,
        }
    }

    pub fn build(self) -> GraphOverlay {
        GraphOverlay {
            action: EmulationAction::FillMount {
                target: self.mount.clone(),
                to_percent: self.to_percent,
            },
            node_overrides: vec![NodeOverlay {
                node_id: self.mount,
                state: OverlayNodeState::HypotheticallyFilled,
                reason: format!(
                    "{} would be hypothetically filled to {}%",
                    self.label, self.to_percent
                ),
            }],
            interrupted_relationships: Vec::new(),
        }
    }
}

pub fn emulate_fill_mount(input: FillMountInput) -> EmulationDomainReport {
    let overlay = FillMountOverlayBuilder::new(
        input.mount.id.clone(),
        input.mount.label.clone(),
        input.to_percent,
    )
    .build();
    let base_nodes = std::slice::from_ref(&input.mount);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);
    let overlay_summary = EmulationOverlaySummary {
        unavailable_nodes: view
            .base_nodes()
            .iter()
            .map(|node| EmulationOverlayNode {
                id: node.id.to_string(),
                label: node.label.clone(),
                reason: format!("hypothetically filled to {}%", input.to_percent),
            })
            .collect(),
        interrupted_relationships: Vec::new(),
    };
    let affected = affected_impacts(&input.affected, &input.mount.label);
    let evidence_lines: Vec<String> = affected.iter().map(|item| item.statement.clone()).collect();
    let (score, evidence_reason) = if input.mount_in_graph && !input.affected.is_empty() {
        (45, "known service paths sit on this mount")
    } else if input.mount_in_graph {
        (15, "no service path is tied to this mount")
    } else {
        (15, "mount is not in the graph")
    };
    let mut risk_reasons = vec![format!("hypothetical fill to {}%", input.to_percent)];
    if let Some(used) = input.current_used_percent {
        risk_reasons.push(format!("mount is currently {used}% used"));
    }
    if !input.mount_in_graph {
        risk_reasons.push("mount is not in the graph; run `twin scan` first".to_string());
    }
    EmulationDomainReport {
        action: FILL_DISK_ACTION.to_string(),
        target: input.mount.id.to_string(),
        target_label: input.mount.label,
        action_performed: false,
        safety_statement: FILL_DISK_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level: fill_risk(input.to_percent),
        risk_reasons,
        evidence_strength: EvidenceStrength::new(score),
        evidence_reasons: vec![evidence_reason.to_string()],
        overlay,
        overlay_summary,
        transient_impacts: Vec::new(),
        configured_impacts: Vec::new(),
        runtime_impacts: Vec::new(),
        restart_impacts: Vec::new(),
        persistent_impacts: affected,
        unknown_impacts: input
            .unknowns
            .iter()
            .map(|unknown| EmulationImpact {
                id: unknown.kind.clone(),
                label: unknown.detail.clone(),
                statement: unknown.detail.clone(),
                path: String::new(),
                evidence: Vec::new(),
            })
            .collect(),
        evidence_lines,
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 0,
    }
}

pub fn fill_risk(to_percent: u8) -> RiskLevel {
    if to_percent >= 80 {
        RiskLevel::Critical
    } else if to_percent >= 50 {
        RiskLevel::High
    } else {
        RiskLevel::Medium
    }
}

fn affected_impacts(affected: &[FillAffectedService], mount_label: &str) -> Vec<EmulationImpact> {
    affected
        .iter()
        .map(|item| {
            let statement = if item.logs {
                format!("{} writes to {}", item.label, item.path)
            } else {
                format!("{} uses {}", item.label, item.path)
            };
            EmulationImpact {
                id: item.id.to_string(),
                label: item.label.clone(),
                statement: statement.clone(),
                path: format!(
                    "{} {} {}",
                    item.id,
                    if item.logs { "logs_to" } else { "uses" },
                    mount_label
                ),
                evidence: vec![statement],
            }
        })
        .collect()
}
