mod support;

use std::path::PathBuf;

use twin_app::{GraphRequest, InitRequest, ScanRequest};
use twin_collectors::COLLECTOR_NAME;
use twin_core::{NodeId, NodeKind};
use twin_store::Store;

use support::IsolatedHome;

fn fixture_proc(home: &support::IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(PathBuf::from("/usr/lib/systemd/systemd").as_path()),
    );
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        Some(PathBuf::from("/usr/bin/nginx").as_path()),
        Some("0::/system.slice/nginx.service\n"),
    );
    proc
}

#[test]
fn scan_in_persists_process_nodes_and_parent_edges() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert_eq!(result.process_count, 2);
    assert_eq!(result.parent_edge_count, 1);
    let store = Store::open(&home.layout.db_file()).expect("open");
    assert_eq!(store.count_nodes().expect("nodes"), 6);
    assert!(store.count_edges().expect("edges") >= 1);
}

#[test]
fn scan_in_records_collector_run() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let runs = store
        .list_collector_runs(Some(COLLECTOR_NAME))
        .expect("runs");
    assert_eq!(runs.len(), 1);
    assert!(runs[0].observation_count > 0);
}

#[test]
fn scan_in_repeated_scan_updates_last_seen() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan1");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let first = store.get_node("process:pid:42").expect("get").expect("row");
    std::thread::sleep(std::time::Duration::from_millis(2));
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan2");
    let second = store.get_node("process:pid:42").expect("get").expect("row");
    assert_eq!(second.first_seen_ns, first.first_seen_ns);
    assert!(second.last_seen_ns >= first.last_seen_ns);
}

#[test]
fn graph_in_lists_process_nodes() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Process),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::List(list) = result else {
        panic!("expected list");
    };
    assert_eq!(list.kind, NodeKind::Process);
    assert_eq!(list.nodes.len(), 2);
}

#[test]
fn graph_in_returns_process_neighborhood() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let target = NodeId::process(42);
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(target),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Node(node) = result else {
        panic!("expected node");
    };
    assert_eq!(node.incoming.len(), 1);
    assert_eq!(node.incoming[0].peer_id, "process:pid:1");
}

#[test]
fn scan_in_persists_service_and_cgroup_graph() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert_eq!(result.service_count, 1);
    assert_eq!(result.cgroup_count, 1);
    assert!(result.in_cgroup_edge_count >= 1);
    assert!(result.service_owns_edge_count >= 2);

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("service:nginx.service")
        .expect("get")
        .is_some());
    assert!(store
        .get_node("cgroup:/system.slice/nginx.service")
        .expect("get")
        .is_some());
    let owns = store
        .list_edges_by_kind("owns")
        .expect("owns")
        .into_iter()
        .filter(|e| e.from_node_id.starts_with("service:"))
        .count();
    assert!(owns >= 2);
}

#[test]
fn graph_in_lists_service_nodes() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Service),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::List(list) = result else {
        panic!("expected list");
    };
    assert_eq!(list.kind, NodeKind::Service);
    assert_eq!(list.nodes.len(), 1);
}

#[test]
fn graph_in_service_neighborhood_by_query() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target_query: Some("nginx".to_string()),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Service(service) = result else {
        panic!("expected service view");
    };
    assert_eq!(service.service.id, "service:nginx.service");
    assert!(!service.owned_processes.is_empty());
    assert!(!service.evidence.is_empty());
}

#[test]
fn graph_in_service_target_by_id() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("nginx.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    assert!(matches!(result, twin_app::GraphResult::Service(_)));
}

#[test]
fn graph_in_rejects_unsupported_port_kind() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Port),
            ..GraphRequest::default()
        },
    )
    .expect_err("unsupported");
    assert!(matches!(
        err,
        twin_app::AppError::UnsupportedGraphKind { .. }
    ));
}

#[test]
fn scan_json_includes_warning_details() {
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture(&proc, 9, "not valid stat", "", b"", None);
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(!result.warning_details.is_empty());
    assert!(!result.warnings.is_empty());
}

#[test]
fn graph_in_evidence_includes_ppid_line() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::process(42)),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Node(node) = result else {
        panic!("expected node view");
    };
    assert!(
        node.evidence_refs
            .iter()
            .any(|line| line.contains("/proc/42/stat") && line.contains("ppid=1")),
        "expected formatted parent evidence, got {:?}",
        node.evidence_refs
    );
}
