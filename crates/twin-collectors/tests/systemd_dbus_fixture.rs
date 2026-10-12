use tempfile::TempDir;
use twin_collectors::{SystemdRuntimeCollector, UnitDBusSnapshot};
use twin_core::TimestampNs;
use twin_observation::ObservationKind;

#[test]
fn runtime_collector_uses_dbus_fixture() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let units = vec![UnitDBusSnapshot {
        name: "docker.service".to_string(),
        active_state: "active".to_string(),
        load_state: "loaded".to_string(),
        sub_state: "running".to_string(),
        triggered_by: vec!["api.socket".into()],
        invocation_id: Some("0123456789abcdef0123456789abcdef".to_string()),
        requires: vec!["containerd.service".to_string()],
        wants: vec![],
        control_group: "/system.slice/docker.service".to_string(),
        fragment_path: "/usr/lib/systemd/system/docker.service".to_string(),
    }];
    let batch = SystemdRuntimeCollector::with_dbus_fixture(vec![root], units)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.dbus_available());
    assert_eq!(batch.dbus_unit_count(), 1);
    assert!(batch.observations().iter().any(|o| {
        o.kind == ObservationKind::SystemdUnitRequires
            && o.metadata.get("from_unit").and_then(|v| v.as_str()) == Some("docker.service")
            && o.metadata.get("to_unit").and_then(|v| v.as_str()) == Some("containerd.service")
            && o.metadata.get("source").and_then(|v| v.as_str()) == Some("systemd_dbus")
    }));
}

#[test]
fn invocation_identity_is_safe_runtime_metadata() {
    let units = vec![UnitDBusSnapshot {
        name: "api.service".into(),
        active_state: "active".into(),
        load_state: "loaded".into(),
        sub_state: "running".into(),
        triggered_by: vec!["api.socket".into()],
        invocation_id: Some("0123456789abcdef0123456789abcdef".into()),
        requires: vec![],
        wants: vec![],
        control_group: String::new(),
        fragment_path: String::new(),
    }];
    let batch = SystemdRuntimeCollector::with_dbus_fixture(vec![], units)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let observation = batch
        .observations()
        .iter()
        .find(|obs| obs.kind == ObservationKind::SystemdUnitStateSeen)
        .expect("state");
    assert_eq!(
        observation
            .metadata
            .get("invocation_id")
            .and_then(|value| value.as_str()),
        Some("0123456789abcdef0123456789abcdef")
    );
}

#[test]
fn runtime_socket_trigger_relations_name_observed_connection_instances() {
    let units = vec![UnitDBusSnapshot {
        name: "api@connection-42.service".into(),
        active_state: "active".into(),
        load_state: "loaded".into(),
        sub_state: "running".into(),
        invocation_id: None,
        triggered_by: vec!["api.socket".into(), "not-a-socket.target".into()],
        requires: vec![],
        wants: vec![],
        control_group: String::new(),
        fragment_path: String::new(),
    }];
    let batch = SystemdRuntimeCollector::with_dbus_fixture(vec![], units)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let activations: Vec<_> = batch
        .observations()
        .iter()
        .filter(|obs| obs.kind == ObservationKind::SystemdSocketActivates)
        .collect();
    assert_eq!(activations.len(), 1);
    assert_eq!(
        activations[0]
            .metadata
            .get("service_unit")
            .and_then(|value| value.as_str()),
        Some("api@connection-42.service")
    );
    assert_eq!(
        activations[0]
            .metadata
            .get("socket_unit")
            .and_then(|value| value.as_str()),
        Some("api.socket")
    );
    assert_eq!(
        activations[0].raw_ref.as_ref().expect("ref").as_str(),
        "org.freedesktop.systemd1.Unit.TriggeredBy"
    );
}

fn socket_unit(name: &str) -> UnitDBusSnapshot {
    UnitDBusSnapshot {
        name: name.into(),
        active_state: "active".into(),
        load_state: "loaded".into(),
        sub_state: "listening".into(),
        invocation_id: None,
        triggered_by: vec![],
        requires: vec![],
        wants: vec![],
        control_group: String::new(),
        fragment_path: String::new(),
    }
}

#[test]
fn socket_listen_configuration_is_scoped_and_does_not_create_ownership() {
    use std::collections::HashMap;
    use twin_collectors::{SocketListenAddress, SocketListenKind};
    let fifo =
        SocketListenAddress::new(SocketListenKind::Fifo, "/run/api.fifo".into()).expect("fifo");
    let vsock =
        SocketListenAddress::new(SocketListenKind::Stream, "vsock:2:9000".into()).expect("vsock");
    let batch = SystemdRuntimeCollector::with_dbus_fixture(
        vec![],
        vec![socket_unit("api.socket"), socket_unit("hidden.socket")],
    )
    .with_socket_listen_fixture(HashMap::from([("api.socket".into(), vec![fifo, vsock])]))
    .collect(TimestampNs::new(1))
    .expect("collect");
    let observations = batch.observations();
    assert_eq!(observations.len(), 2);
    assert!(observations
        .iter()
        .all(|obs| obs.kind == ObservationKind::SystemdUnitStateSeen && obs.object.is_none()));
    let known = observations
        .iter()
        .find(|obs| obs.metadata.get("unit").and_then(|v| v.as_str()) == Some("api.socket"))
        .expect("api");
    assert_eq!(
        known
            .metadata
            .get("socket_config_known")
            .and_then(|v| v.as_bool()),
        Some(true)
    );
    assert_eq!(
        known.metadata.get("socket_config_fifos").expect("fifos"),
        &serde_json::json!(["/run/api.fifo"])
    );
    assert_eq!(
        known
            .metadata
            .get("socket_config_streams")
            .expect("streams"),
        &serde_json::json!(["vsock:2:9000"])
    );
    let unknown = observations
        .iter()
        .find(|obs| obs.metadata.get("unit").and_then(|v| v.as_str()) == Some("hidden.socket"))
        .expect("hidden");
    assert_eq!(
        unknown
            .metadata
            .get("socket_config_known")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(unknown.metadata.get("socket_config_fifos").is_none());
    assert_eq!(batch.warnings().len(), 1);
    assert_eq!(
        batch.warnings()[0].kind(),
        twin_collectors::SystemdWarningKind::SocketPropertyUnreadable
    );
    assert_eq!(batch.warnings()[0].path().to_str(), Some("hidden.socket"));
}

#[test]
fn socket_listen_is_bounded_and_rejects_control_characters() {
    use std::collections::HashMap;
    use twin_collectors::{SocketListenAddress, SocketListenKind};
    for address in ["".to_string(), "x".repeat(4097), "bad\naddress".into()] {
        assert!(SocketListenAddress::new(SocketListenKind::Fifo, address).is_err());
    }
    let entry =
        SocketListenAddress::new(SocketListenKind::Stream, "vsock:2:9000".into()).expect("address");
    let batch = SystemdRuntimeCollector::with_dbus_fixture(vec![], vec![socket_unit("api.socket")])
        .with_socket_listen_fixture(HashMap::from([("api.socket".into(), vec![entry; 257])]))
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.warnings().len(), 1);
    assert_eq!(
        batch.observations()[0]
            .metadata
            .get("socket_config_known")
            .and_then(|v| v.as_bool()),
        Some(false)
    );
    assert!(batch.observations()[0]
        .metadata
        .get("socket_config_streams")
        .is_none());
}
