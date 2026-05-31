use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, Pipeline, RawIdentity,
    RawObservation, RedactionState,
};

#[test]
fn end_to_end() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("DATABASE_URL", "postgres://user:pass@host:5432/db");
    let raw = RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessSeen,
        collector: CollectorName::new("proc"),
        subject: Some(RawIdentity::Process { pid: 8841 }),
        object: None,
        timestamp: TimestampNs::new(99),
        raw_ref: None,
        confidence_hint: ConfidenceHint::High,
        metadata,
    };
    let obs = Pipeline::default().process(raw).expect("pipeline");
    assert_eq!(obs.subject().map(|n| n.as_str()), Some("process:pid:8841"));
    assert_eq!(obs.redaction_state(), RedactionState::Redacted);
    assert_eq!(
        obs.metadata().get("DATABASE_URL").and_then(|v| v.as_str()),
        Some("<redacted>")
    );
}

#[test]
fn no_identities() {
    let raw = RawObservation {
        source: ObservationSource::ProcCgroup,
        kind: ObservationKind::ProcessBelongsToCgroup,
        collector: CollectorName::new("proc"),
        subject: None,
        object: None,
        timestamp: TimestampNs::new(1),
        raw_ref: None,
        confidence_hint: ConfidenceHint::Moderate,
        metadata: ObservationMetadata::new(),
    };
    let obs = Pipeline::default().process(raw).expect("pipeline");
    assert!(obs.subject().is_none());
    assert!(obs.object().is_none());
}
