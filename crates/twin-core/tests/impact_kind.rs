use std::str::FromStr;

use twin_core::{DependentImpactKind, UnknownKind};

#[test]
fn dependent_impact_kind_display_and_parse() {
    assert_eq!(DependentImpactKind::Runtime.to_string(), "runtime");
    assert_eq!(
        DependentImpactKind::from_str("configured").unwrap(),
        DependentImpactKind::Configured
    );
    assert!(DependentImpactKind::Configured.is_runtime() == false);
    assert!(DependentImpactKind::Runtime.is_runtime());
}

#[test]
fn unknown_kind_display_and_parse() {
    assert_eq!(UnknownKind::MissingEvidence.to_string(), "missing_evidence");
    assert_eq!(
        UnknownKind::from_str("unmapped_active_sockets").unwrap(),
        UnknownKind::UnmappedActiveSockets
    );
    assert_eq!(
        UnknownKind::UnmappedListenerSockets.human_label(),
        "unmapped listeners"
    );
}
