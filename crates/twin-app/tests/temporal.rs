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
