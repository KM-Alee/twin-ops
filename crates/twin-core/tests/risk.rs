use std::str::FromStr;

use twin_core::{EvidenceLabel, EvidenceStrength, RiskLevel};

#[test]
fn risk_level_display_from_str_roundtrip() {
    for level in [
        RiskLevel::Low,
        RiskLevel::Medium,
        RiskLevel::High,
        RiskLevel::Critical,
        RiskLevel::Unknown,
    ] {
        let s = level.to_string();
        assert_eq!(RiskLevel::from_str(&s).expect("parse"), level);
    }
}

#[test]
fn evidence_label_display_from_str_roundtrip() {
    for label in [
        EvidenceLabel::Weak,
        EvidenceLabel::Moderate,
        EvidenceLabel::Strong,
        EvidenceLabel::VeryStrong,
    ] {
        let s = label.to_string();
        assert_eq!(EvidenceLabel::from_str(&s).expect("parse"), label);
    }
}

#[test]
fn evidence_strength_label_boundaries() {
    assert_eq!(EvidenceStrength::new(0).label(), EvidenceLabel::Weak);
    assert_eq!(EvidenceStrength::new(30).label(), EvidenceLabel::Weak);
    assert_eq!(EvidenceStrength::new(31).label(), EvidenceLabel::Moderate);
    assert_eq!(EvidenceStrength::new(60).label(), EvidenceLabel::Moderate);
    assert_eq!(EvidenceStrength::new(61).label(), EvidenceLabel::Strong);
    assert_eq!(EvidenceStrength::new(85).label(), EvidenceLabel::Strong);
    assert_eq!(EvidenceStrength::new(86).label(), EvidenceLabel::VeryStrong);
    assert_eq!(
        EvidenceStrength::new(100).label(),
        EvidenceLabel::VeryStrong
    );
}

#[test]
fn evidence_strength_clamps_out_of_range() {
    assert_eq!(EvidenceStrength::new(255).score(), 100);
}

#[test]
fn serde_uses_lowercase_labels() {
    let risk = serde_json::to_value(RiskLevel::High).expect("json");
    assert_eq!(risk, "high");
    let label = serde_json::to_value(EvidenceLabel::VeryStrong).expect("json");
    assert_eq!(label, "very_strong");
    let strength = serde_json::to_value(EvidenceStrength::moderate()).expect("json");
    let obj = strength.as_object().expect("object");
    assert_eq!(obj.get("score").and_then(|v| v.as_u64()), Some(45));
    assert_eq!(obj.get("label").and_then(|v| v.as_str()), Some("moderate"));
}
