use twin_core::{EvidenceStrength, NodeId, NodeState, RiskLevel};

use crate::action::{EmulationAction, SAFETY_STATEMENT, UPGRADE_ACTION, UPGRADE_SAFETY_STATEMENT};
use crate::effective_view::EffectiveGraphView;
use crate::input::{EmulationNode, EmulationUnknown};
use crate::overlay::{GraphOverlay, NodeOverlay, OverlayNodeState};
use crate::report::{
    EmulationDomainReport, EmulationImpact, EmulationOverlayNode, EmulationOverlaySummary,
};

#[derive(Debug, Clone)]
pub struct UpgradeAffectedService {
    pub id: NodeId,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct UpgradePackageInput {
    pub package: EmulationNode,
    pub package_in_graph: bool,
    pub mapped_stems: Vec<String>,
    pub affected: Vec<UpgradeAffectedService>,
    pub unknowns: Vec<EmulationUnknown>,
}

pub struct UpgradePackageOverlayBuilder {
    package: NodeId,
    label: String,
}

impl UpgradePackageOverlayBuilder {
    pub fn new(package: NodeId, label: impl Into<String>) -> Self {
        Self {
            package,
            label: label.into(),
        }
    }

    pub fn build(self) -> GraphOverlay {
        GraphOverlay {
            action: EmulationAction::UpgradePackage {
                target: self.package.clone(),
            },
            node_overrides: vec![NodeOverlay {
                node_id: self.package,
                state: OverlayNodeState::HypotheticallyUpgraded,
                reason: format!(
                    "{} would be hypothetically upgraded; no package manager ran",
                    self.label
                ),
            }],
            interrupted_relationships: Vec::new(),
        }
    }
}

pub fn emulate_upgrade_package(input: UpgradePackageInput) -> EmulationDomainReport {
    let overlay =
        UpgradePackageOverlayBuilder::new(input.package.id.clone(), input.package.label.clone())
            .build();
    let base_nodes = std::slice::from_ref(&input.package);
    let view = EffectiveGraphView::new(base_nodes, NodeState::Active, &overlay);
    let overlay_summary = EmulationOverlaySummary {
        unavailable_nodes: view
            .base_nodes()
            .iter()
            .map(|node| EmulationOverlayNode {
                id: node.id.to_string(),
                label: node.label.clone(),
                reason: "hypothetical package upgrade".to_string(),
            })
            .collect(),
        interrupted_relationships: Vec::new(),
    };

    let affected = if input.package_in_graph {
        input.affected
    } else {
        Vec::new()
    };
    let stems = if input.package_in_graph {
        input.mapped_stems
    } else {
        Vec::new()
    };
    let runtime_low = !stems.is_empty();
    let restart_high = !affected.is_empty();
    let runtime_level = if runtime_low { "LOW" } else { "UNKNOWN" };
    let restart_level = if restart_high {
        "HIGH"
    } else if input.package_in_graph {
        "LOW"
    } else {
        "UNKNOWN"
    };
    let runtime_reason = runtime_reason(&stems, input.package_in_graph);
    let (score, evidence_reason, risk_level) = if !input.package_in_graph {
        (15, "package is not in the graph", RiskLevel::Unknown)
    } else if restart_high && runtime_low {
        (
            45,
            "service dependency inferred from mapped libraries",
            RiskLevel::High,
        )
    } else if restart_high {
        (
            15,
            "dependent services are recorded without a mapped library",
            RiskLevel::High,
        )
    } else {
        (15, "no service depends on this package", RiskLevel::Low)
    };

    let mut unknowns = input.unknowns;
    if !input.package_in_graph
        && !unknowns
            .iter()
            .any(|unknown| unknown.detail.contains("not in the graph"))
    {
        unknowns.push(EmulationUnknown {
            kind: "missing_evidence".to_string(),
            detail: "target package is not in the graph; run `twin scan` first".to_string(),
            source: None,
            weakens_evidence: true,
        });
    }

    EmulationDomainReport {
        action: UPGRADE_ACTION.to_string(),
        target: input.package.id.to_string(),
        target_label: input.package.label,
        action_performed: false,
        safety_statement: UPGRADE_SAFETY_STATEMENT.to_string(),
        general_safety_statement: SAFETY_STATEMENT.to_string(),
        risk_level,
        risk_reasons: vec![format!("restart impact {restart_level}")],
        evidence_strength: EvidenceStrength::new(score),
        evidence_reasons: vec![evidence_reason.to_string()],
        overlay,
        overlay_summary,
        transient_impacts: Vec::new(),
        configured_impacts: Vec::new(),
        runtime_impacts: vec![EmulationImpact {
            id: input.package.id.to_string(),
            label: runtime_level.to_string(),
            statement: runtime_reason,
            path: String::new(),
            evidence: Vec::new(),
        }],
        restart_impacts: affected
            .iter()
            .map(|service| EmulationImpact {
                id: service.id.to_string(),
                label: service.label.clone(),
                statement: format!(
                    "{} loads a library from this package and would pick it up on restart",
                    service.label
                ),
                path: format!("{} depends_on {}", service.id, input.package.id),
                evidence: Vec::new(),
            })
            .collect(),
        persistent_impacts: Vec::new(),
        unknown_impacts: unknowns
            .iter()
            .map(|unknown| EmulationImpact {
                id: unknown.kind.clone(),
                label: unknown.detail.clone(),
                statement: unknown.detail.clone(),
                path: String::new(),
                evidence: Vec::new(),
            })
            .collect(),
        evidence_lines: affected
            .iter()
            .map(|service| format!("{} depends_on {}", service.label, input.package.id))
            .collect(),
        impact_paths: Vec::new(),
        paths_requested: false,
        max_depth: 0,
    }
}

fn runtime_reason(stems: &[String], package_in_graph: bool) -> String {
    if !package_in_graph {
        return "package is not in the graph; run `twin scan` first".to_string();
    }
    if stems.is_empty() {
        return "no running process has a library from this package mapped".to_string();
    }
    format!(
        "running processes already have current {} mapped.",
        stems.join(", ")
    )
}
