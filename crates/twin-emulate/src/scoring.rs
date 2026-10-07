use twin_core::{
    factors_from_lines, score_capped_evidence, EvidenceAdjustments, EvidenceExplanation,
    EvidenceLineRef, EvidenceRecency, RiskLevel,
};

use crate::input::{
    EmulationConfiguredService, EmulationDependent, EmulationEvidenceLine, EmulationUnknown,
    RestartPathScoringInput,
};

const RESTART_RISK_MEDIUM_MAX: usize = 2;

pub const REASON_NO_RUNTIME_DEPENDENTS: &str =
    "no runtime dependents were found in the current graph";
pub const REASON_COVERAGE_GAPS: &str = "coverage gaps may hide additional dependents";

pub fn score_restart_risk(
    runtime_dependents: &[EmulationDependent],
    configured_dependents: &[EmulationDependent],
    unknowns: &[EmulationUnknown],
    path_scoring: Option<&RestartPathScoringInput>,
) -> (RiskLevel, Vec<String>) {
    let significant_unknowns = unknowns.iter().any(|u| u.weakens_evidence);
    let count = runtime_dependents.len();
    let mut reasons = Vec::new();

    let mut level = match count {
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
    if let Some(path) = path_scoring {
        if path.transitive_runtime_count > 0 {
            reasons.push(format!(
                "{} transitive runtime dependent(s) in blast-radius paths",
                path.transitive_runtime_count
            ));
            level = raise_restart_risk(level);
        }
        if let Some(label) = &path.public_exposure_label {
            reasons.push(format!(
                "dependency path reaches non-loopback listener on {label}"
            ));
            level = raise_restart_risk(level);
        }
        for hint in &path.criticality_hints {
            reasons.push(hint.clone());
        }
        if !path.criticality_hints.is_empty() && count > 0 {
            level = raise_restart_risk(level);
        }
    }

    (level, reasons)
}

fn raise_restart_risk(level: RiskLevel) -> RiskLevel {
    match level {
        RiskLevel::Low => RiskLevel::Medium,
        RiskLevel::Medium => RiskLevel::High,
        RiskLevel::High => RiskLevel::Critical,
        other => other,
    }
}

pub fn score_restart_evidence(
    runtime_dependents: &[EmulationDependent],
    unknowns: &[EmulationUnknown],
    path_evidence: &[EmulationEvidenceLine],
    adjustments: EvidenceAdjustments,
) -> EvidenceExplanation {
    let mut has_observation_links = false;
    let mut only_inferred = true;
    let mut lines = Vec::new();

    for dependent in runtime_dependents {
        if !dependent.observation_ids.is_empty() {
            has_observation_links = true;
        }
        if dependent.edge_class != twin_core::EdgeClass::Inferred {
            only_inferred = false;
        }
        push_emulation_lines(&mut lines, &dependent.evidence);
    }
    push_emulation_lines(&mut lines, path_evidence);
    let factors = factors_from_lines(&lines, adjustments);

    score_capped_evidence(
        factors,
        has_observation_links,
        only_inferred,
        unknowns.iter().any(|u| u.weakens_evidence),
        !runtime_dependents.is_empty() || !path_evidence.is_empty(),
    )
}

pub fn score_delete_risk(
    configured_services: &[EmulationConfiguredService],
    file_in_graph: bool,
    file_exists: bool,
    unknowns: &[EmulationUnknown],
) -> (RiskLevel, Vec<String>) {
    let significant_unknowns = unknowns.iter().any(|u| u.weakens_evidence);
    let mut reasons = Vec::new();
    let count = configured_services.len();

    let level = match count {
        0 if !file_in_graph => {
            reasons.push("target file is not in the graph".to_string());
            if significant_unknowns {
                reasons.push(REASON_COVERAGE_GAPS.to_string());
            }
            RiskLevel::Unknown
        }
        0 => {
            reasons.push("no configured services were found for this file".to_string());
            if !file_exists {
                reasons.push("file existence could not be confirmed".to_string());
            }
            if significant_unknowns {
                reasons.push(REASON_COVERAGE_GAPS.to_string());
            }
            RiskLevel::Low
        }
        1 => {
            reasons.push("one service is configured by this file".to_string());
            RiskLevel::High
        }
        _ => {
            reasons.push(format!("{count} services are configured by this file"));
            RiskLevel::Critical
        }
    };

    if count == 1 && configured_services.iter().any(|s| !s.evidence.is_empty()) {
        reasons.push("restart impact may be critical for configured service".to_string());
        if level == RiskLevel::High {
            return (RiskLevel::Critical, reasons);
        }
    }

    (level, reasons)
}

pub fn score_delete_evidence(
    configured_services: &[EmulationConfiguredService],
    evidence: &[EmulationEvidenceLine],
    unknowns: &[EmulationUnknown],
    adjustments: EvidenceAdjustments,
) -> EvidenceExplanation {
    let mut lines = Vec::new();
    push_emulation_lines(&mut lines, evidence);
    for service in configured_services {
        push_emulation_lines(&mut lines, &service.evidence);
    }
    let has_links = !configured_services.is_empty() || !evidence.is_empty();
    let factors = factors_from_lines(&lines, adjustments);
    score_capped_evidence(
        factors,
        has_links,
        false,
        unknowns.iter().any(|u| u.weakens_evidence),
        has_links,
    )
}

fn push_emulation_lines<'a>(
    out: &mut Vec<EvidenceLineRef<'a>>,
    lines: &'a [EmulationEvidenceLine],
) {
    for line in lines {
        out.push(EvidenceLineRef {
            source: &line.source,
            statement: &line.statement,
            relationship: &line.relationship,
            strength_score: line.strength_score,
        });
    }
}

pub fn adjustments_from(
    permission_gaps: u32,
    dropped_ebpf: bool,
    recency: EvidenceRecency,
) -> EvidenceAdjustments {
    EvidenceAdjustments {
        permission_gaps,
        dropped_ebpf,
        recency,
        conflicting: false,
    }
}
