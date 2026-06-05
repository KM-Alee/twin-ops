mod support;

use std::path::PathBuf;

use twin_app::{GraphRequest, ImpactRequest, InitRequest, ScanRequest};
use twin_collectors::COLLECTOR_NAME;
use twin_core::{NodeId, NodeKind};
use twin_store::Store;

use support::{write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, IsolatedHome};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

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

fn fixture_proc_with_listener(home: &IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc-listener");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        None,
        None,
    );
    write_proc_fixture_with_cgroup(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    proc
}

#[test]
fn scan_in_persists_port_and_listener_edges() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert_eq!(result.tcp_listener_count, 1);
    assert_eq!(result.port_count, 1);
    assert!(
        result.socket_owner_inode_count >= 1,
        "socket_owner_inode_count=0; probe had owners"
    );
    assert!(
        result.process_listens_on_edge_count >= 1,
        "tcp_listeners={} ports={} unmapped={} owner_inodes={} warnings={:?}",
        result.tcp_listener_count,
        result.port_count,
        result.unmapped_listener_socket_count,
        result.socket_owner_inode_count,
        result.warnings
    );
    assert!(result.service_listens_on_edge_count >= 1);

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("port:tcp:127.0.0.1:5432")
        .expect("get")
        .is_some());
    let listens: Vec<_> = store
        .list_edges_by_kind("listens_on")
        .expect("edges")
        .into_iter()
        .filter(|e| e.to_node_id == "port:tcp:127.0.0.1:5432")
        .collect();
    assert!(listens.len() >= 2);
}

#[test]
fn graph_in_port_neighborhood() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let port = NodeId::port_tcp("127.0.0.1", 5432).expect("port");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(port),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Port(view) = result else {
        panic!("expected port view");
    };
    assert!(!view.process_listeners.is_empty());
    assert!(!view.service_listeners.is_empty());
    assert!(!view.evidence.is_empty());
}

#[test]
fn graph_in_lists_port_nodes() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Port),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::List(list) = result else {
        panic!("expected list");
    };
    assert_eq!(list.kind, NodeKind::Port);
    assert_eq!(list.nodes.len(), 1);
}

#[test]
fn graph_in_service_shows_listening_ports() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("postgresql.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Service(service) = result else {
        panic!("expected service");
    };
    assert!(!service.listening_ports.is_empty());
    assert_eq!(service.listening_ports[0].id, "port:tcp:127.0.0.1:5432");
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

fn fixture_proc_with_active_connection(home: &IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc-deps");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    write_socket_fd(&proc, 8841, 12, 456);
    proc
}

#[test]
fn scan_in_persists_active_connection_edges() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert_eq!(result.tcp_connection_count, 1);
    assert!(result.process_connects_to_edge_count >= 1);
    assert!(result.service_connects_to_edge_count >= 1);

    let store = Store::open(&home.layout.db_file()).expect("open");
    let connects: Vec<_> = store
        .list_edges_by_kind("connects_to")
        .expect("edges")
        .into_iter()
        .filter(|e| e.to_node_id == "port:tcp:127.0.0.1:5432")
        .collect();
    assert!(connects.len() >= 2);
}

#[test]
fn scan_in_infers_service_dependency() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(result.service_depends_on_edge_count >= 1);

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(
        store
            .get_edge("service:django.service|depends_on|service:postgresql.service")
            .expect("get")
            .is_some()
            || store
                .list_edges_by_kind("depends_on")
                .expect("edges")
                .iter()
                .any(|e| {
                    e.from_node_id == "service:django.service"
                        && e.to_node_id == "service:postgresql.service"
                })
    );
}

#[test]
fn graph_in_service_shows_dependencies_and_dependents() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");

    let django = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("django.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph django");
    let twin_app::GraphResult::Service(django_view) = django else {
        panic!("expected service");
    };
    assert!(!django_view.connected_ports.is_empty());
    assert!(!django_view.dependencies.is_empty());

    let postgres = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("postgresql.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph postgres");
    let twin_app::GraphResult::Service(postgres_view) = postgres else {
        panic!("expected service");
    };
    assert!(!postgres_view.dependents.is_empty());
}

#[test]
fn graph_in_port_shows_callers() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let port = NodeId::port_tcp("127.0.0.1", 5432).expect("port");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(port),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Port(view) = result else {
        panic!("expected port");
    };
    assert!(!view.service_callers.is_empty() || !view.process_callers.is_empty());
}

#[test]
fn impact_in_service_shows_direct_dependents() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("postgresql".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(!result.direct_dependents.is_empty());
    assert!(result
        .direct_dependents
        .iter()
        .any(|d| d.id == "service:django.service"));
}

#[test]
fn impact_in_port_shows_direct_callers() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::port_tcp("127.0.0.1", 5432).expect("port")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(!result.direct_dependents.is_empty());
}

#[test]
fn scan_in_infers_dependency_via_wildcard_listener() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-wildcard-listen");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    write_socket_fd(&proc, 8841, 12, 456);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(result.service_depends_on_edge_count >= 1);
    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .list_edges_by_kind("depends_on")
        .expect("edges")
        .iter()
        .any(|e| {
            e.from_node_id == "service:django.service"
                && e.to_node_id == "service:postgresql.service"
        }));
}

#[test]
fn unmapped_active_connection_does_not_create_dependency() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-unmapped-active");
    std::fs::create_dir_all(&proc).expect("proc");
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(result.unmapped_active_socket_count >= 1);
    assert_eq!(result.service_depends_on_edge_count, 0);
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
