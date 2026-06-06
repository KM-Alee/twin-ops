use twin_core::{cap_dependent_evidence_score, EvidenceStrength, RiskLevel};

use crate::input::{EmulationDependent, EmulationUnknown};

const RESTART_RISK_MEDIUM_MAX: usize = 2;

pub const REASON_NO_RUNTIME_DEPENDENTS: &str =
    "no runtime dependents were found in the current graph";
pub const REASON_COVERAGE_GAPS: &str = "coverage gaps may hide additional dependents";

pub fn score_restart_risk(
    runtime_dependents: &[EmulationDependent],
    configured_dependents: &[EmulationDependent],
    unknowns: &[EmulationUnknown],
) -> (RiskLevel, Vec<String>) {
    let significant_unknowns = unknowns.iter().any(|u| u.weakens_evidence);
    let count = runtime_dependents.len();
    let mut reasons = Vec::new();

    let level = match count {
        0 if !significant_unknowns => {
            reasons.push(REASON_NO_RUNTIME_DEPENDENTS.to_string());
            if !configured_dependents.is_empty() {
                reasons.push(format!(
                    "{} configured dependent(s) may be affected after restart or reload",
                    configured_dependents.len()
                ));
            }
            RiskLevel::Low
        }
        0 => {
            reasons.push(REASON_NO_RUNTIME_DEPENDENTS.to_string());
            reasons.push(REASON_COVERAGE_GAPS.to_string());
            RiskLevel::Unknown
        }
        1..=RESTART_RISK_MEDIUM_MAX => {
            reasons.push(format!(
                "{count} direct runtime dependent(s) may be interrupted"
            ));
            RiskLevel::Medium
        }
        _ => {
            reasons.push(format!(
                "{count} direct runtime dependents may be interrupted"
            ));
            RiskLevel::High
        }
    };

    if count > 0 && significant_unknowns {
        reasons.push(REASON_COVERAGE_GAPS.to_string());
    }

    (level, reasons)
}

pub fn score_restart_evidence(
    runtime_dependents: &[EmulationDependent],
    unknowns: &[EmulationUnknown],
) -> EvidenceStrength {
    let mut best = 0u8;
    let mut has_observation_links = false;
    let mut only_inferred = true;

    for dependent in runtime_dependents {
        if !dependent.observation_ids.is_empty() {
            has_observation_links = true;
        }
        if dependent.edge_class != twin_core::EdgeClass::Inferred {
            only_inferred = false;
        }
        for line in &dependent.evidence {
            best = best.max(line.strength_score);
        }
    }

    cap_dependent_evidence_score(
        best,
        has_observation_links,
        only_inferred,
        unknowns.iter().any(|u| u.weakens_evidence),
        !runtime_dependents.is_empty(),
    )
}
