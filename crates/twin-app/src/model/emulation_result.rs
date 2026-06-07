use serde::Serialize;
use twin_emulate::EmulationDomainReport;

use super::impact_result::{EvidenceStrengthView, ImpactUnknown, RiskAssessment};

#[derive(Debug, Clone, Serialize)]
pub struct EmulationOverlayNode {
    pub id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulationOverlayInterrupted {
    pub edge_id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulationOverlaySummary {
    pub unavailable_nodes: Vec<EmulationOverlayNode>,
    pub interrupted_relationships: Vec<EmulationOverlayInterrupted>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulationImpact {
    pub id: String,
    pub label: String,
    pub statement: String,
    pub path: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulationImpactPathView {
    pub terminal_id: String,
    pub terminal_label: String,
    pub depth: usize,
    pub path_chain: String,
    pub evidence: Vec<String>,
    pub is_cycle_capped: bool,
    pub is_depth_capped: bool,
    pub cycle_note: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EmulationResult {
    pub action: String,
    pub target: String,
    pub target_label: String,
    pub action_performed: bool,
    pub safety_statement: String,
    pub risk: RiskAssessment,
    pub evidence_strength: EvidenceStrengthView,
    pub overlay: EmulationOverlaySummary,
    pub transient_impacts: Vec<EmulationImpact>,
    pub configured_impacts: Vec<EmulationImpact>,
    pub runtime_impacts: Vec<EmulationImpact>,
    pub restart_impacts: Vec<EmulationImpact>,
    pub persistent_impacts: Vec<EmulationImpact>,
    pub unknown_impacts: Vec<EmulationImpact>,
    pub evidence_lines: Vec<String>,
    pub general_safety_statement: String,
    pub unknowns: Vec<ImpactUnknown>,
    #[serde(default)]
    pub impact_paths: Vec<EmulationImpactPathView>,
    #[serde(default)]
    pub paths_requested: bool,
    #[serde(default)]
    pub max_depth: usize,
}

impl EmulationResult {
    pub fn from_domain(report: EmulationDomainReport, unknowns: Vec<ImpactUnknown>) -> Self {
        Self {
            action: report.action,
            target: report.target,
            target_label: report.target_label,
            action_performed: report.action_performed,
            safety_statement: report.safety_statement,
            risk: RiskAssessment {
                level: report.risk_level,
                reasons: report.risk_reasons,
            },
            evidence_strength: EvidenceStrengthView::from(report.evidence_strength),
            overlay: EmulationOverlaySummary {
                unavailable_nodes: report
                    .overlay_summary
                    .unavailable_nodes
                    .into_iter()
                    .map(|n| EmulationOverlayNode {
                        id: n.id,
                        label: n.label,
                        reason: n.reason,
                    })
                    .collect(),
                interrupted_relationships: report
                    .overlay_summary
                    .interrupted_relationships
                    .into_iter()
                    .map(|r| EmulationOverlayInterrupted {
                        edge_id: r.edge_id,
                        from: r.from,
                        to: r.to,
                        kind: r.kind,
                        reason: r.reason,
                    })
                    .collect(),
            },
            transient_impacts: report
                .transient_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            configured_impacts: report
                .configured_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            runtime_impacts: report
                .runtime_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            restart_impacts: report
                .restart_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            persistent_impacts: report
                .persistent_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            unknown_impacts: report
                .unknown_impacts
                .into_iter()
                .map(|i| EmulationImpact {
                    id: i.id,
                    label: i.label,
                    statement: i.statement,
                    path: i.path,
                    evidence: i.evidence,
                })
                .collect(),
            evidence_lines: report.evidence_lines,
            general_safety_statement: report.general_safety_statement,
            unknowns,
            impact_paths: report
                .impact_paths
                .into_iter()
                .map(|p| EmulationImpactPathView {
                    terminal_id: p.terminal_id,
                    terminal_label: p.terminal_label,
                    depth: p.depth,
                    path_chain: p.path_chain,
                    evidence: p.evidence,
                    is_cycle_capped: p.is_cycle_capped,
                    is_depth_capped: p.is_depth_capped,
                    cycle_note: p.cycle_note,
                })
                .collect(),
            paths_requested: report.paths_requested,
            max_depth: report.max_depth,
        }
    }
}
