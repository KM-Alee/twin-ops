use serde::{Deserialize, Serialize};
use twin_core::RiskLevel;

use super::impact_result::EvidenceStrengthView;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreflightVerdict {
    Go,
    Caution,
    Stop,
}

impl PreflightVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Go => "go",
            Self::Caution => "caution",
            Self::Stop => "stop",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightAffected {
    pub id: String,
    pub label: String,
    pub effect: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightStep {
    pub segment: String,
    pub operation: String,
    pub target: Option<String>,
    pub verdict: PreflightVerdict,
    pub is_assessed: bool,
    pub risk: Option<RiskLevel>,
    pub evidence: Option<EvidenceStrengthView>,
    pub reasons: Vec<String>,
    pub affected: Vec<PreflightAffected>,
    pub affected_total: usize,
    pub advice: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreflightResult {
    pub command: String,
    pub verdict: PreflightVerdict,
    pub headline: String,
    pub has_operations: bool,
    pub steps: Vec<PreflightStep>,
    pub graph_age_secs: Option<u64>,
    pub coverage_notes: Vec<String>,
    pub action_performed: bool,
}
