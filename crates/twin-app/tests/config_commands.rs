mod support;

use twin_app::{GraphRequest, InitRequest, ScanRequest, SnapshotListRequest};
use twin_core::{ChangeKind, TimestampNs};
use twin_store::{NodeRow, ObservationRow, Store};

#[test]
fn explicit_bad_config_is_rejected_before_scan_writes() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let path = home.layout.data_dir.join("bad.toml");
    std::fs::write(&path, "[retention").unwrap();
    let request = ScanRequest {
        config_override: Some(path.clone()),
        ..ScanRequest::default()
    };
    assert!(twin_app::scan_in(
        &home.layout,
        request,
        &home.layout.data_dir.join("absent-proc")
    )
    .is_err());
    assert!(Store::open(&home.layout.db_file())
        .unwrap()
        .list_node_history_since(0)
        .unwrap()
        .is_empty());
    assert!(twin_app::graph_in(
        &home.layout,
        GraphRequest {
            config_override: Some(path.clone()),
            ..GraphRequest::default()
        }
    )
    .is_err());
    assert!(twin_app::snapshot_list_in(
        &home.layout,
        SnapshotListRequest {
            config_override: Some(path)
        }
    )
    .is_err());
    assert!(twin_app::snapshot_list_in(
        &home.layout,
        SnapshotListRequest {
            config_override: Some(home.layout.data_dir.join("missing.toml"))
        }
    )
    .is_err());
}

#[test]
fn scan_applies_custom_retention_without_relocating_database() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let day = 86_400_000_000_000;
    let now = TimestampNs::now().as_i64();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    for (id, source, age) in [
        ("proc-1", "proc", 1),
        ("proc-3", "proc", 3),
        ("ebpf-1", "ebpf", 1),
        ("ebpf-3", "ebpf", 3),
    ] {
        store
            .insert_observation(&ObservationRow {
                id: id.into(),
                source: source.into(),
                kind: "test".into(),
                subject_node_id: None,
                object_node_id: None,
                timestamp_ns: now - age * day,
                confidence_hint: "low".into(),
                redaction_state: "none".into(),
                metadata_json: format!("{{\"fixture\":\"{id}\"}}"),
                collector_run_id: None,
                last_seen_ns: now - age * day,
                seen_count: 1,
            })
            .unwrap();
    }
    let node = NodeRow {
        id: "process:pid:9".into(),
        kind: "process".into(),
        label: "old".into(),
        state: "active".into(),
        first_seen_ns: now,
        last_seen_ns: now,
        valid_from_ns: now,
        valid_to_ns: None,
        metadata_json: "{}".into(),
    };
    store
        .insert_node_history(&node, ChangeKind::New, now - 5 * day, None)
        .unwrap();
    store
        .insert_node_history(&node, ChangeKind::Changed, now - 4 * day, None)
        .unwrap();
    store
        .insert_node_history(&node, ChangeKind::Changed, now - 2 * day, None)
        .unwrap();
    let path = home.layout.data_dir.join("project.toml");
    std::fs::write(
        &path,
        "[retention]\nraw_observations_days = 2\nebpf_events_days = 4\ngraph_history_days = 3\n",
    )
    .unwrap();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).unwrap();
    twin_app::scan_in(
        &home.layout,
        ScanRequest {
            config_override: Some(path),
            ..ScanRequest::default()
        },
        &proc,
    )
    .unwrap();
    let ids: Vec<_> = store
        .list_observations_by_kind("test")
        .unwrap()
        .into_iter()
        .map(|row| row.id)
        .collect();
    assert_eq!(ids.len(), 3);
    assert!(!ids.contains(&"proc-3".to_string()));
    let history = store.list_node_history_since(0).unwrap();
    assert!(history
        .iter()
        .any(|row| row.recorded_at_ns == now - 2 * day));
    assert!(
        history
            .iter()
            .any(|row| row.recorded_at_ns == now - 4 * day),
        "the newest row before the cutoff is the state at the cutoff"
    );
    assert!(!history
        .iter()
        .any(|row| row.recorded_at_ns == now - 5 * day));
    assert!(!home.layout.data_dir.join("twin.db").is_dir());
}
