use std::str::FromStr;

use twin_core::{CollectorName, ObservationId, ParseError, TimestampNs};

#[test]
fn timestamp_roundtrip() {
    let ts = TimestampNs::new(42);
    assert_eq!(ts.as_i64(), 42);
    assert!(TimestampNs::now().as_i64() >= 0);
}

#[test]
fn observation_id_str_roundtrip() {
    let id = ObservationId::new();
    let s = id.to_string();
    let parsed = ObservationId::from_str(&s).expect("parse");
    assert_eq!(parsed, id);
}

#[test]
fn collector_name_roundtrip() {
    let name = CollectorName::new("proc");
    let parsed = CollectorName::from_str(name.as_str()).expect("parse");
    assert_eq!(parsed, name);
}

#[test]
fn observation_id_bad_string_errors() {
    let err = ObservationId::from_str("not-a-uuid").expect_err("bad");
    assert!(matches!(err, ParseError::ObservationId { .. }));
}
