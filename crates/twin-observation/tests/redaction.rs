use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    BasicRedactor, ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource,
    RawObservation, RedactionState, Redactor,
};

fn raw_with_metadata(metadata: ObservationMetadata) -> RawObservation {
    RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessSeen,
        collector: CollectorName::new("proc"),
        subject: None,
        object: None,
        timestamp: TimestampNs::new(1),
        raw_ref: None,
        confidence_hint: ConfidenceHint::Moderate,
        metadata,
    }
}

#[test]
fn connection_string() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("DATABASE_URL", "postgres://user:pass@host:5432/db");
    let mut raw = raw_with_metadata(metadata);
    let state = BasicRedactor.redact(&mut raw);
    assert_eq!(state, RedactionState::Redacted);
    assert_eq!(
        raw.metadata.get("DATABASE_URL").and_then(|v| v.as_str()),
        Some("<redacted>")
    );
    assert_eq!(
        raw.metadata.get("maybe_endpoint").and_then(|v| v.as_str()),
        Some("host:5432")
    );
    assert_eq!(
        raw.metadata.get("value_redacted").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn sensitive_keys() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("api_token", "sekrit");
    metadata.insert_str("user_password", "hunter2");
    let mut raw = raw_with_metadata(metadata);
    let state = BasicRedactor.redact(&mut raw);
    assert_eq!(state, RedactionState::Redacted);
    assert_eq!(
        raw.metadata.get("api_token").and_then(|v| v.as_str()),
        Some("<redacted>")
    );
    assert_eq!(
        raw.metadata.get("value_redacted").and_then(|v| v.as_bool()),
        Some(true)
    );
}

#[test]
fn connection_string_without_credentials() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("DATABASE_URL", "postgres://host:5432/db");
    let mut raw = raw_with_metadata(metadata);
    let state = BasicRedactor.redact(&mut raw);
    assert_eq!(state, RedactionState::Redacted);
    assert_eq!(
        raw.metadata.get("DATABASE_URL").and_then(|v| v.as_str()),
        Some("<redacted>")
    );
    assert_eq!(
        raw.metadata.get("maybe_endpoint").and_then(|v| v.as_str()),
        Some("host:5432")
    );
}

#[test]
fn file_url_not_redacted() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("path", "file:///etc/app.conf");
    let mut raw = raw_with_metadata(metadata);
    assert_eq!(BasicRedactor.redact(&mut raw), RedactionState::None);
}

#[test]
fn keeps_safe_fields() {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("env_name", "PATH");
    metadata.insert_str("config_path", "/etc/app.conf");
    metadata.insert_str("listen_addr", "127.0.0.1:8080");
    let mut raw = raw_with_metadata(metadata);
    let state = BasicRedactor.redact(&mut raw);
    assert_eq!(state, RedactionState::None);
    assert_eq!(
        raw.metadata.get("env_name").and_then(|v| v.as_str()),
        Some("PATH")
    );
}
