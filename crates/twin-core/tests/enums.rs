use std::str::FromStr;

use twin_core::{EdgeClass, EdgeKind, EdgeState, NodeKind, NodeState, ParseError};

fn roundtrip<T>(variants: &[T])
where
    T: std::fmt::Display + std::str::FromStr<Err = ParseError> + PartialEq + std::fmt::Debug + Copy,
{
    for variant in variants {
        let s = variant.to_string();
        let parsed = T::from_str(&s).expect("parse");
        assert_eq!(*variant, parsed, "round-trip failed for {s}");
    }
}

#[test]
fn all_variants_roundtrip() {
    roundtrip(&[
        NodeKind::Host,
        NodeKind::Process,
        NodeKind::Service,
        NodeKind::Port,
        NodeKind::File,
        NodeKind::Cgroup,
        NodeKind::Mount,
        NodeKind::Directory,
        NodeKind::Library,
        NodeKind::Package,
        NodeKind::Container,
        NodeKind::Image,
        NodeKind::K8sNamespace,
        NodeKind::K8sPod,
        NodeKind::K8sDeployment,
        NodeKind::K8sReplicaSet,
        NodeKind::K8sService,
        NodeKind::K8sEndpoint,
        NodeKind::K8sConfigMap,
        NodeKind::K8sSecretRef,
        NodeKind::K8sPvc,
        NodeKind::K8sIngress,
        NodeKind::K8sEvent,
    ]);
    roundtrip(&[NodeState::Active, NodeState::Stale, NodeState::Gone]);
    roundtrip(&[
        EdgeKind::ParentOf,
        EdgeKind::Owns,
        EdgeKind::InCgroup,
        EdgeKind::ListensOn,
        EdgeKind::ConnectsTo,
        EdgeKind::ConfiguredBy,
        EdgeKind::DependsOn,
        EdgeKind::ProxiesTo,
        EdgeKind::References,
        EdgeKind::MountedOn,
        EdgeKind::LogsTo,
        EdgeKind::Uses,
        EdgeKind::LoadsLibrary,
        EdgeKind::InstalledBy,
        EdgeKind::RunsImage,
        EdgeKind::MapsPort,
        EdgeKind::MountsVolume,
        EdgeKind::Selects,
        EdgeKind::RoutesTo,
        EdgeKind::UsesConfigMap,
        EdgeKind::UsesSecretRef,
        EdgeKind::UsesPvc,
    ]);
    roundtrip(&[
        EdgeClass::Observed,
        EdgeClass::Inferred,
        EdgeClass::Predicted,
    ]);
    roundtrip(&[EdgeState::Active, EdgeState::Stale, EdgeState::Gone]);
}

#[test]
fn unknown_enum_errors() {
    let err = NodeKind::from_str("unknown").expect_err("bad");
    assert!(matches!(
        err,
        ParseError::Enum {
            kind: "NodeKind",
            ..
        }
    ));
}
