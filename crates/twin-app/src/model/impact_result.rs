use serde::Serialize;
use twin_core::EvidenceStrength;

use super::graph_result::GraphOwnedNode;

#[derive(Debug, Clone, Serialize)]
pub struct ImpactNodeSummary {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactPathStep {
    pub from: ImpactNodeSummary,
    pub edge_kind: String,
    pub edge_class: String,
    pub to: ImpactNodeSummary,
    pub edge_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactEvidenceLine {
    pub source: String,
    pub statement: String,
    pub relationship: String,
    pub strength: String,
    pub observation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactDependent {
    pub id: String,
    pub label: String,
    pub relationship: String,
    pub edge_class: String,
    pub impact_kind: String,
    pub reason: String,
    pub path: Vec<ImpactPathStep>,
    pub evidence: Vec<ImpactEvidenceLine>,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactUnknown {
    pub kind: String,
    pub detail: String,
    pub source: Option<String>,
    pub weakens_evidence: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct RiskAssessment {
    pub level: String,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EvidenceStrengthView {
    pub score: u8,
    pub label: String,
}

impl From<EvidenceStrength> for EvidenceStrengthView {
    fn from(value: EvidenceStrength) -> Self {
        Self {
            score: value.score(),
            label: value.label().to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    pub target: String,
    pub target_label: String,
    pub risk: RiskAssessment,
    pub evidence_strength: EvidenceStrengthView,
    pub direct_dependents: Vec<ImpactDependent>,
    pub configured_dependents: Vec<ImpactDependent>,
    pub listener_owners: Vec<GraphOwnedNode>,
    pub evidence: Vec<ImpactEvidenceLine>,
    pub unknowns: Vec<ImpactUnknown>,
}
