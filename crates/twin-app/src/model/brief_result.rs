use serde::{Deserialize, Serialize};

use super::preflight_result::PreflightVerdict;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefFact {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub label: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefSection {
    pub title: String,
    pub items: Vec<BriefItem>,
    pub omitted: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefRestart {
    pub verdict: PreflightVerdict,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BriefResult {
    pub target: Option<String>,
    pub title: String,
    pub kind: String,
    pub facts: Vec<BriefFact>,
    pub restart: Option<BriefRestart>,
    pub sections: Vec<BriefSection>,
    pub graph_age_secs: u64,
    pub coverage_notes: Vec<String>,
    pub next_steps: Vec<String>,
}
