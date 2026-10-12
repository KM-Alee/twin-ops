use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WhyObservation {
    pub id: String,
    pub source: String,
    pub kind: String,
    pub timestamp_ns: i64,
    pub raw_ref: Option<String>,
    pub facts: Vec<String>,
    pub repeat_count: usize,
    pub first_timestamp_ns: i64,
}

impl WhyObservation {
    pub fn timestamp_label(&self) -> String {
        timestamp_label(self.timestamp_ns)
    }

    pub fn first_timestamp_label(&self) -> String {
        timestamp_label(self.first_timestamp_ns)
    }
}

fn timestamp_label(timestamp_ns: i64) -> String {
    chrono::DateTime::from_timestamp(
        timestamp_ns.div_euclid(1_000_000_000),
        timestamp_ns.rem_euclid(1_000_000_000) as u32,
    )
    .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
    .unwrap_or_else(|| format!("{timestamp_ns} ns since epoch"))
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
