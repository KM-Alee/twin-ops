use std::str::FromStr;

use twin_core::{EdgeId, EdgeKind, NodeId, ParseError};

#[test]
fn new_format_is_deterministic() {
    let from = NodeId::process(1);
    let to = NodeId::process(2);
    let id = EdgeId::new(&from, EdgeKind::DependsOn, &to);
    assert_eq!(id.as_str(), "process:pid:1|depends_on|process:pid:2");
}

#[test]
fn from_str_roundtrip() {
    let from = NodeId::host("localhost");
    let to = NodeId::service("nginx.service");
    let id = EdgeId::new(&from, EdgeKind::ListensOn, &to);
    let parsed = EdgeId::from_str(id.as_str()).expect("parse");
    assert_eq!(parsed, id);
}

#[test]
fn from_str_rejects_garbage() {
    let err = EdgeId::from_str("not-an-edge").expect_err("bad");
    assert!(matches!(err, ParseError::InvalidEdgeId { .. }));
}
