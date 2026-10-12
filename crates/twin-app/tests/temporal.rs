mod support;

use twin_app::{
    DiffRequest, InitRequest, ScanRequest, SnapshotCreateRequest, SnapshotListRequest,
    WhatChangedRequest,
};
use twin_core::ParseError;
use twin_store::Store;

use support::IsolatedHome;

fn scan_fixture(home: &IsolatedHome, proc: &std::path::Path) {
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    twin_app::scan_in(&home.layout, ScanRequest::default(), proc).expect("scan");
}

#[test]
fn repeated_scan_records_new_and_disappeared_process() {
    let home = IsolatedHome::new();
    let proc_v1 = home.layout.data_dir.join("proc-v1");
    std::fs::create_dir_all(&proc_v1).expect("proc");
    support::write_proc_fixture(
        &proc_v1,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    support::write_proc_fixture(
        &proc_v1,
        42,
        "42 (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/worker\0",
        Some(std::path::Path::new("/usr/bin/worker")),
    );

    scan_fixture(&home, &proc_v1);

    let proc_v2 = home.layout.data_dir.join("proc-v2");
    std::fs::create_dir_all(&proc_v2).expect("proc");
    support::write_proc_fixture(
        &proc_v2,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc_v2).expect("scan2");

    let result = twin_app::what_changed_in(
        &home.layout,
        WhatChangedRequest {
            target_query: None,
            categories: vec![],
            limit: 50,
            max_depth: 2,
            since: "1h".to_string(),
            config_override: None,
            verbose: false,
        },
    )
    .expect("what-changed");
    assert!(
        result
            .disappeared_nodes
            .iter()
            .any(|n| n.id == "process:pid:42"),
        "expected disappeared process: {:?}",
        result.disappeared_nodes
    );
}

#[test]
fn unchanged_repeated_scan_does_not_duplicate_changed_rows() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    scan_fixture(&home, &proc);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan2");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let history = store.list_node_history_since(0).expect("history");
    let changed = history
        .iter()
        .filter(|row| row.change_kind == "changed")
        .count();
    assert_eq!(
        changed, 0,
        "unchanged scans should not flood changed history"
    );
}

#[test]
fn snapshot_create_and_list_roundtrip() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    scan_fixture(&home, &proc);
    let created = twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "before-change".to_string(),
            config_override: None,
        },
    )
    .expect("create");
    assert_eq!(created.name, "before-change");
    let listed =
        twin_app::snapshot_list_in(&home.layout, SnapshotListRequest::default()).expect("list");
    assert_eq!(listed.snapshots.len(), 1);
    assert_eq!(listed.snapshots[0].node_count, created.node_count);
}

#[test]
fn snapshot_rejects_invalid_or_duplicate_names() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    scan_fixture(&home, &proc);

    let err = twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "current".to_string(),
            config_override: None,
        },
    )
    .expect_err("reserved");
    assert!(err.to_string().contains("reserved"));

    let err = twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "bad:name".to_string(),
            config_override: None,
        },
    )
    .expect_err("colon");
    assert!(err.to_string().contains("invalid"));

    twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "ok-name".to_string(),
            config_override: None,
        },
    )
    .expect("first");
    let err = twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "ok-name".to_string(),
            config_override: None,
        },
    )
    .expect_err("duplicate");
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn diff_snapshot_to_current_reports_added_removed_changed() {
    let home = IsolatedHome::new();
    let proc_v1 = home.layout.data_dir.join("proc-v1");
    std::fs::create_dir_all(&proc_v1).expect("proc");
    support::write_proc_fixture(
        &proc_v1,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    support::write_proc_fixture(
        &proc_v1,
        42,
        "42 (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/worker\0",
        Some(std::path::Path::new("/usr/bin/worker")),
    );
    scan_fixture(&home, &proc_v1);
    twin_app::snapshot_create_in(
        &home.layout,
        SnapshotCreateRequest {
            name: "checkpoint".to_string(),
            config_override: None,
        },
    )
    .expect("snapshot");

    let proc_v2 = home.layout.data_dir.join("proc-v2");
    std::fs::create_dir_all(&proc_v2).expect("proc");
    support::write_proc_fixture(
        &proc_v2,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(std::path::Path::new("/usr/lib/systemd/systemd")),
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc_v2).expect("scan2");

    let diff = twin_app::diff_in(
        &home.layout,
        DiffRequest {
            left: "snapshot:checkpoint".to_string(),
            right: "current".to_string(),
            config_override: None,
        },
    )
    .expect("diff");
    assert!(
        diff.nodes_removed.iter().any(|n| n.id == "process:pid:42"),
        "expected removed node: {:?}",
        diff.nodes_removed
    );
}

#[test]
fn diff_rejects_unknown_snapshot_ref() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::diff_in(
        &home.layout,
        DiffRequest {
            left: "snapshot:missing".to_string(),
            right: "current".to_string(),
            config_override: None,
        },
    )
    .expect_err("missing snapshot");
    assert!(err.to_string().contains("not found"));
}

#[test]
fn what_changed_rejects_invalid_duration() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::what_changed_in(
        &home.layout,
        WhatChangedRequest {
            target_query: None,
            categories: vec![],
            limit: 50,
            max_depth: 2,
            since: "0m".to_string(),
            config_override: None,
            verbose: false,
        },
    )
    .expect_err("zero duration");
    assert!(err.to_string().contains("greater than zero"));
}

#[test]
fn parse_graph_ref_rejects_malformed_values() {
    let err = twin_core::parse_graph_ref("snapshot:").expect_err("empty name");
    assert!(matches!(err, ParseError::GraphRef { .. }));
}

#[test]
fn changed_history_reports_fields_and_handles_pruned_predecessor() {
    use twin_core::{ChangeKind, TimestampNs};
    use twin_store::NodeRow;
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let now = TimestampNs::now().as_i64();
    let mut node = NodeRow {
        id: "service:api.service".into(),
        kind: "service".into(),
        label: "before".into(),
        state: "active".into(),
        first_seen_ns: now,
        last_seen_ns: now,
        valid_from_ns: now,
        valid_to_ns: None,
        metadata_json: "{}".into(),
    };
    store
        .insert_node_history(&node, ChangeKind::New, now - 2, None)
        .unwrap();
    node.label = "after".into();
    node.metadata_json = r#"{"hash":"new"}"#.into();
    store
        .insert_node_history(&node, ChangeKind::Changed, now - 1, None)
        .unwrap();
    let request = WhatChangedRequest {
        target_query: None,
        categories: vec![],
        limit: 50,
        max_depth: 2,
        since: "1h".into(),
        config_override: None,
        verbose: false,
    };
    let result = twin_app::what_changed_in(&home.layout, request.clone()).unwrap();
    assert_eq!(
        result.changed_nodes[0].summary,
        "label before -> after, metadata changed"
    );
    store.prune_retention(0, 0, now - 1).unwrap();
    let result = twin_app::what_changed_in(&home.layout, request).unwrap();
    assert_eq!(
        result.changed_nodes[0].summary,
        "previous history unavailable"
    );
}

#[test]
fn historical_graph_and_list_filters_use_retained_state_without_changing_current_graph() {
    use twin_app::{GraphRequest, GraphResult};
    use twin_core::{ChangeKind, NodeKind, NodeState, TimestampNs};
    use twin_store::NodeRow;
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let mut node = NodeRow {
        id: "process:pid:1".into(),
        kind: "process".into(),
        label: "before".into(),
        state: "active".into(),
        first_seen_ns: 100,
        last_seen_ns: 100,
        valid_from_ns: 100,
        valid_to_ns: None,
        metadata_json: "{}".into(),
    };
    store
        .insert_node_history(&node, ChangeKind::New, 100, None)
        .unwrap();
    node.label = "after".into();
    node.state = "stale".into();
    store
        .insert_node_history(&node, ChangeKind::Stale, 200, None)
        .unwrap();
    store.upsert_node(&node).unwrap();
    let GraphResult::List(view) = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Process),
            at: Some(TimestampNs::new(100)),
            ..GraphRequest::default()
        },
    )
    .unwrap() else {
        panic!("list");
    };
    assert_eq!(view.nodes[0].label, "before");
    let GraphResult::List(view) = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Process),
            state: Some(NodeState::Stale),
            label: Some("aft".into()),
            limit: Some(1),
            ..GraphRequest::default()
        },
    )
    .unwrap() else {
        panic!("list");
    };
    assert_eq!(view.nodes.len(), 1);
    assert_eq!(view.nodes[0].label, "after");
    assert_eq!(
        store.get_node("process:pid:1").unwrap().unwrap().state,
        "stale"
    );
    assert!(twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Process),
            at: Some(TimestampNs::new(50)),
            ..GraphRequest::default()
        }
    )
    .is_err());
}

#[test]
fn graph_time_parser_requires_real_past_time() {
    assert_eq!(
        twin_app::parse_graph_time("1970-01-01T00:00:01Z")
            .unwrap()
            .as_i64(),
        1_000_000_000
    );
    assert!(twin_app::parse_graph_time("1h").is_ok());
    for raw in [
        "invalid",
        "0h",
        "2100-01-01T00:00:00Z",
        "2026-02-30T00:00:00Z",
    ] {
        assert!(twin_app::parse_graph_time(raw).is_err(), "{raw}");
    }
}

#[test]
fn operational_changes_use_retained_transitions_and_never_invent_restart_causes() {
    use twin_core::{ChangeCategory, ChangeKind, EdgeClass, TimestampNs};
    use twin_store::{EdgeRow, NodeRow};
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let now = TimestampNs::now().as_i64();
    let fixtures = [
        (
            "service:api.service",
            "service",
            r#"{"invocation_id":"11111111111111111111111111111111"}"#,
            r#"{"invocation_id":"22222222222222222222222222222222"}"#,
        ),
        (
            "file:/etc/api.conf",
            "file",
            r#"{"content_hash":"old"}"#,
            r#"{"content_hash":"new","password":"do-not-print"}"#,
        ),
        (
            "package:openssl",
            "package",
            r#"{"version":"1.0"}"#,
            r#"{"version":"2.0"}"#,
        ),
        (
            "k8s:deployment:default/api",
            "k8s_deployment",
            r#"{"ready_pods":1}"#,
            r#"{"ready_pods":2}"#,
        ),
    ];
    for (id, kind, before, after) in fixtures {
        let mut node = NodeRow {
            id: id.into(),
            kind: kind.into(),
            label: id.into(),
            state: "active".into(),
            first_seen_ns: now - 10,
            last_seen_ns: now - 10,
            valid_from_ns: now - 10,
            valid_to_ns: None,
            metadata_json: before.into(),
        };
        store
            .insert_node_history(&node, ChangeKind::New, now - 10, None)
            .expect("before");
        node.metadata_json = after.into();
        store
            .insert_node_history(&node, ChangeKind::Changed, now - 5, None)
            .expect("after");
        // The final state matching the initial state must not erase the intervening change.
        if kind == "package" {
            node.metadata_json = before.into();
            store
                .insert_node_history(&node, ChangeKind::Changed, now - 1, None)
                .expect("rollback");
        }
    }
    for (id, kind, change) in [
        ("process:pid:12", "process", ChangeKind::New),
        ("process:pid:13", "process", ChangeKind::Gone),
        ("container:api", "container", ChangeKind::New),
    ] {
        let node = NodeRow {
            id: id.into(),
            kind: kind.into(),
            label: id.into(),
            state: "active".into(),
            first_seen_ns: now - 5,
            last_seen_ns: now - 5,
            valid_from_ns: now - 5,
            valid_to_ns: None,
            metadata_json: "{}".into(),
        };
        store
            .insert_node_history(&node, change, now - 5, None)
            .expect("history");
    }
    for (kind, target, change) in [
        ("listens_on", "port:tcp:127.0.0.1:80", ChangeKind::New),
        ("listens_on", "port:udp:127.0.0.1:53", ChangeKind::Gone),
        ("connects_to", "port:tcp:192.0.2.1:443", ChangeKind::New),
        ("runs_image", "image:api:v2", ChangeKind::New),
    ] {
        let edge = EdgeRow {
            id: format!("service:api.service|{kind}|{target}"),
            from_node_id: "service:api.service".into(),
            to_node_id: target.into(),
            kind: kind.into(),
            class: "inferred".into(),
            state: "active".into(),
            evidence_score: 40,
            evidence_label: "moderate".into(),
            evidence_count: 1,
            first_seen_ns: now - 5,
            last_seen_ns: now - 5,
            metadata_json: "{}".into(),
        };
        let mut edge = edge;
        if change == ChangeKind::Gone {
            store
                .insert_edge_history(&edge, ChangeKind::New, now - 8, None)
                .expect("listener before");
            edge.state = "gone".into();
        }
        store
            .insert_edge_history(&edge, change, now - 5, None)
            .expect("edge history");
    }
    let result = twin_app::what_changed_in(
        &home.layout,
        WhatChangedRequest {
            target_query: None,
            categories: vec![],
            limit: 50,
            max_depth: 2,
            since: "1h".into(),
            config_override: None,
            verbose: false,
        },
    )
    .expect("changes");
    for category in [
        ChangeCategory::ProcessStarted,
        ChangeCategory::ProcessDisappeared,
        ChangeCategory::ServiceRestarted,
        ChangeCategory::ConfigChanged,
        ChangeCategory::ListeningPortAdded,
        ChangeCategory::ListeningPortDisappeared,
        ChangeCategory::OutboundEndpointAdded,
        ChangeCategory::ContainerAdded,
        ChangeCategory::ImageChanged,
        ChangeCategory::PackageVersionChanged,
        ChangeCategory::KubernetesObjectChanged,
    ] {
        assert!(
            result
                .operational_changes
                .iter()
                .any(|change| change.category == category),
            "{category}"
        );
    }
    assert_eq!(
        result
            .operational_changes
            .iter()
            .filter(|change| change.category == ChangeCategory::PackageVersionChanged)
            .count(),
        2
    );
    let restart = result
        .operational_changes
        .iter()
        .find(|change| change.category == ChangeCategory::ServiceRestarted)
        .expect("restart");
    assert_eq!(restart.class, EdgeClass::Inferred);
    assert!(restart.observation.contains("restart cause unknown"));
    assert!(restart.source.contains("node_history:"));
    assert!(!serde_json::to_string(&result)
        .expect("JSON")
        .contains("do-not-print"));
}

#[test]
fn malformed_or_zero_invocation_ids_do_not_establish_restart_evidence() {
    use twin_core::{ChangeCategory, ChangeKind};
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let now = twin_core::TimestampNs::now().as_i64();
    for (index, value) in [
        "",
        "arbitrary-private-value",
        "00000000000000000000000000000000",
        "gggggggggggggggggggggggggggggggg",
    ]
    .into_iter()
    .enumerate()
    {
        for reverse in [false, true] {
            let mut node = twin_store::NodeRow {
                id: format!("service:invalid-{index}-{reverse}.service"),
                kind: "service".into(),
                label: "invalid".into(),
                state: "active".into(),
                first_seen_ns: now - 10,
                last_seen_ns: now - 10,
                valid_from_ns: now - 10,
                valid_to_ns: None,
                metadata_json: "{}".into(),
            };
            let valid = "11111111111111111111111111111111";
            for (offset, kind, identity) in [
                (10, ChangeKind::New, if reverse { valid } else { value }),
                (5, ChangeKind::Changed, if reverse { value } else { valid }),
            ] {
                node.metadata_json = serde_json::json!({"invocation_id": identity}).to_string();
                store
                    .insert_node_history(&node, kind, now - offset, None)
                    .unwrap();
            }
        }
    }
    let report = twin_app::what_changed_in(
        &home.layout,
        WhatChangedRequest {
            target_query: None,
            categories: vec![],
            limit: 50,
            max_depth: 2,
            since: "1h".into(),
            config_override: None,
            verbose: false,
        },
    )
    .unwrap();
    assert!(!report
        .operational_changes
        .iter()
        .any(|change| change.category == ChangeCategory::ServiceRestarted));
}
