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
