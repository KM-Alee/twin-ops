use serde::Serialize;
use twin_core::{ChangeCategory, EdgeClass, EvidenceStrength, NodeId};

#[derive(Debug, Clone, Serialize)]
pub struct OperationalChange {
    pub category: ChangeCategory,
    pub entity: NodeId,
    pub recorded_at_ns: i64,
    pub class: EdgeClass,
    pub observation: String,
    pub source: String,
    pub evidence_strength: EvidenceStrength,
}

impl OperationalChange {
    pub fn recorded_at_label(&self) -> String {
        chrono::DateTime::from_timestamp(
            self.recorded_at_ns.div_euclid(1_000_000_000),
            self.recorded_at_ns.rem_euclid(1_000_000_000) as u32,
        )
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Millis, true))
        .unwrap_or_else(|| format!("{} ns since epoch", self.recorded_at_ns))
    }
}
