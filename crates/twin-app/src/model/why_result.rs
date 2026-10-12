use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WhyObservation {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub timestamp_ns: i64,
    pub raw_ref: Option<String>,
    pub facts: Vec<String>,
}

impl WhyObservation {
    pub fn timestamp_label(&self) -> String {
        chrono::DateTime::from_timestamp(
            self.timestamp_ns.div_euclid(1_000_000_000),
            self.timestamp_ns.rem_euclid(1_000_000_000) as u32,
        )
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| format!("{} ns since epoch", self.timestamp_ns))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyProofChain {
    pub nodes: Vec<twin_core::NodeId>,
    pub relationship_ids: Vec<String>,
    pub class: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyRelationship {
    pub id: String,
    pub from: String,
    pub to: String,
    pub kind: String,
    pub class: String,
    pub evidence_score: u8,
    pub evidence_strength: String,
    pub observations: Vec<WhyObservation>,
    pub unknowns: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WhyResult {
    pub proof_chains: Vec<WhyProofChain>,
    pub target: String,
    pub label: String,
    pub relationships: Vec<WhyRelationship>,
    pub unknowns: Vec<String>,
}
