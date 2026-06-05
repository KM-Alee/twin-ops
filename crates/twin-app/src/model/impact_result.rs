use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct ImpactDependent {
    pub id: String,
    pub label: String,
    pub relationship: String,
    pub edge_class: String,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactEvidenceLine {
    pub source: String,
    pub statement: String,
    pub relationship: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ImpactResult {
    pub target: String,
    pub target_label: String,
    pub direct_dependents: Vec<ImpactDependent>,
    pub evidence: Vec<ImpactEvidenceLine>,
    pub unknowns: Vec<String>,
}
