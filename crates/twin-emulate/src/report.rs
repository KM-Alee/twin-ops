use twin_core::{EvidenceStrength, RiskLevel};

use crate::overlay::GraphOverlay;

#[derive(Debug, Clone)]
pub struct EmulationOverlayNode {
    pub id: String,
    pub label: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct EmulationOverlayInterrupted {
    pub edge_id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct EmulationOverlaySummary {
    pub unavailable_nodes: Vec<EmulationOverlayNode>,
    pub interrupted_relationships: Vec<EmulationOverlayInterrupted>,
}

#[derive(Debug, Clone)]
pub struct EmulationImpact {
    pub id: String,
    pub label: String,
    pub statement: String,
    pub path: String,
    pub evidence: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EmulationImpactPathReport {
    pub terminal_id: String,
    pub terminal_label: String,
    pub depth: usize,
    pub path_chain: String,
    pub evidence: Vec<String>,
    pub is_cycle_capped: bool,
    pub is_depth_capped: bool,
    pub cycle_note: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EmulationDomainReport {
    pub action: String,
    pub target: String,
    pub target_label: String,
    pub action_performed: bool,
    pub safety_statement: String,
    pub risk_level: RiskLevel,
    pub risk_reasons: Vec<String>,
    pub evidence_strength: EvidenceStrength,
    pub evidence_reasons: Vec<String>,
    pub overlay: GraphOverlay,
    pub overlay_summary: EmulationOverlaySummary,
    pub transient_impacts: Vec<EmulationImpact>,
    pub configured_impacts: Vec<EmulationImpact>,
    pub runtime_impacts: Vec<EmulationImpact>,
    pub restart_impacts: Vec<EmulationImpact>,
    pub persistent_impacts: Vec<EmulationImpact>,
    pub unknown_impacts: Vec<EmulationImpact>,
    pub evidence_lines: Vec<String>,
    pub general_safety_statement: String,
    pub impact_paths: Vec<EmulationImpactPathReport>,
    pub paths_requested: bool,
    pub max_depth: usize,
}
