use twin_core::{
    factors_from_lines, score_capped_evidence, EvidenceAdjustments, EvidenceExplanation,
    EvidenceLineRef, NodeKind, RiskLevel,
};

use crate::commands::impact_paths::ImpactPathAnalysis;
use crate::model::{ImpactDependent, ImpactEvidenceLine, ImpactUnknown, RiskAssessment};

pub(crate) struct PathScoringInput {
    pub transitive_runtime_count: usize,
    pub public_exposure_label: Option<String>,
    pub criticality_hints: Vec<String>,
}

impl PathScoringInput {
    pub fn from_path_analysis(analysis: &ImpactPathAnalysis) -> Self {
        Self {
            transitive_runtime_count: analysis.transitive_runtime_count,
            public_exposure_label: analysis.public_exposure_label.clone(),
            criticality_hints: analysis.criticality_hints.clone(),
        }
    }
}

pub(crate) fn score_service_risk(
    direct_dependents: &[ImpactDependent],
    configured_dependents: &[ImpactDependent],
    unknowns: &[ImpactUnknown],
    path_scoring: Option<&PathScoringInput>,
) -> RiskAssessment {
    let significant_unknowns = unknowns.iter().any(|u| u.weakens_evidence);
    let direct_count = direct_dependents.len();
    let transitive_count = path_scoring
        .map(|p| p.transitive_runtime_count)
        .unwrap_or(0);
    let total_impacted = direct_count
        + path_scoring
            .map(|p| p.transitive_runtime_count)
            .unwrap_or(0);
    let mut reasons = Vec::new();

    let mut level = match direct_count {
        0 if !significant_unknowns => {
            reasons.push("no runtime dependents are known".to_string());
            if !configured_dependents.is_empty() {
                reasons.push(format!(
                    "{} configured dependent(s) are inactive or not runtime-active",
                    configured_dependents.len()
                ));
            }
            RiskLevel::Low
        }
        0 => {
            reasons.push("no runtime dependents are known".to_string());
            reasons.push("coverage gaps may hide additional dependents".to_string());
            RiskLevel::Unknown
        }
        1 | 2 => {
            reasons.push(format!("{direct_count} direct runtime dependent(s)"));
            RiskLevel::Medium
        }
        3..=4 => {
            reasons.push(format!("{direct_count} direct runtime dependents"));
            RiskLevel::High
        }
        _ => {
            reasons.push(format!("{direct_count} direct runtime dependents"));
            RiskLevel::Critical
        }
    };

    if transitive_count > 0 {
        reasons.push(format!(
            "{transitive_count} transitive runtime dependent(s)"
        ));
        level = raise_one_level(level);
    }
    if total_impacted >= 5 {
        level = match level {
            RiskLevel::Low | RiskLevel::Medium => RiskLevel::High,
            RiskLevel::High => RiskLevel::Critical,
            other => other,
        };
        reasons.push(format!(
            "{total_impacted} total runtime impacted service(s)"
        ));
    }
    if let Some(path) = path_scoring {
        if let Some(label) = &path.public_exposure_label {
            reasons.push(format!(
                "dependency path reaches non-loopback listener on {label}"
            ));
            level = raise_one_level(level);
        }
        for hint in &path.criticality_hints {
            reasons.push(hint.clone());
        }
        if !path.criticality_hints.is_empty() && direct_count > 0 {
            level = raise_one_level(level);
        }
    }
    if direct_count > 0 && significant_unknowns {
        reasons.push("coverage gaps may hide additional dependents".to_string());
    }
    if direct_count > 0 {
        reasons.push("target is a service dependency target".to_string());
    }

    RiskAssessment { level, reasons }
}

pub(crate) fn score_port_risk_with_paths(
    target_kind: NodeKind,
    direct_dependents: &[ImpactDependent],
    unknowns: &[ImpactUnknown],
    paths_requested: bool,
) -> RiskAssessment {
    let mut assessment = score_service_risk(direct_dependents, &[], unknowns, None);
    if paths_requested && target_kind != NodeKind::Service {
        assessment.reasons.push(
            "recursive service blast-radius traversal applies to service targets only".to_string(),
        );
    }
    assessment
}

fn raise_one_level(level: RiskLevel) -> RiskLevel {
    match level {
        RiskLevel::Low => RiskLevel::Medium,
        RiskLevel::Medium => RiskLevel::High,
        RiskLevel::High => RiskLevel::Critical,
        other => other,
    }
}

pub(crate) fn score_service_evidence(
    direct_dependents: &[ImpactDependent],
    path_evidence: &[ImpactEvidenceLine],
    unknowns: &[ImpactUnknown],
    adjustments: EvidenceAdjustments,
) -> EvidenceExplanation {
    let mut has_observation_links = false;
    let mut only_inferred = true;
    let mut lines = Vec::new();

    for dependent in direct_dependents {
        if !dependent.observation_ids.is_empty() {
            has_observation_links = true;
        }
        if dependent.edge_class != twin_core::EdgeClass::Inferred.to_string() {
            only_inferred = false;
        }
        push_impact_lines(&mut lines, &dependent.evidence);
    }
    push_impact_lines(&mut lines, path_evidence);
    let factors = factors_from_lines(&lines, adjustments);
    score_capped_evidence(
        factors,
        has_observation_links,
        only_inferred,
        unknowns.iter().any(|u| u.weakens_evidence),
        !direct_dependents.is_empty() || !path_evidence.is_empty(),
    )
}

fn push_impact_lines<'a>(out: &mut Vec<EvidenceLineRef<'a>>, lines: &'a [ImpactEvidenceLine]) {
    for line in lines {
        out.push(EvidenceLineRef {
            source: &line.source,
            statement: &line.statement,
            relationship: &line.relationship,
            strength_score: line.strength_score(),
        });
    }
}
