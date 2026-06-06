mod support;

use std::path::PathBuf;

use twin_app::{GraphRequest, ImpactRequest, InitRequest, ScanRequest};
use twin_collectors::COLLECTOR_NAME;
use twin_core::{NodeId, NodeKind, RiskLevel};
use twin_store::Store;

use support::{
    write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, write_unix_table,
    IsolatedHome,
};

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
    let _env = support::lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    let empty_host = tempfile::TempDir::new().expect("empty host root");
    support::set_host_root(empty_host.path());
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    support::clear_scan_env();
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
fn scan_in_twice_does_not_fail_on_edge_observation_links() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan1");
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan2");
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
    assert_eq!(result.risk.level, RiskLevel::Medium);
    assert!(result.evidence_strength.score >= 31);
    assert!(result.direct_dependents.iter().any(|d| {
        d.evidence
            .iter()
            .any(|line| line.statement.contains("Observed:"))
    }));
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
    assert!(result
        .listener_owners
        .iter()
        .any(|o| o.id == "service:postgresql.service"));
    assert!(result
        .direct_dependents
        .iter()
        .any(|d| d.relationship == "connects_to"));
}

#[test]
fn impact_in_service_shorthand_resolves() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("postgresql.service".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert_eq!(result.target, "service:postgresql.service");
}

#[test]
fn impact_in_ambiguous_service_errors() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-ambiguous");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        1,
        "1 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        2,
        "2 (ang) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/ang\0",
        None,
        Some("0::/system.slice/ang.service\n"),
    );
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let err = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("ng".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect_err("ambiguous");
    assert!(err.to_string().contains("ambiguous"));
}

#[test]
fn impact_in_no_dependents_low_risk() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("nginx".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(result.direct_dependents.is_empty());
    assert_eq!(result.risk.level, RiskLevel::Low);
    assert!(
        result.unknowns.iter().all(|u| !u.weakens_evidence),
        "unexpected weakening unknowns: {:?}",
        result.unknowns
    );
}

#[test]
fn impact_in_unmapped_active_surfaces_unknowns() {
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
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::port_tcp("127.0.0.1", 5432).expect("port")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(result
        .unknowns
        .iter()
        .any(|u| u.kind == "unmapped_active_sockets"));
}

#[test]
fn impact_in_multiple_dependents_raises_risk() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .upsert_node(&twin_store::NodeRow {
            id: "service:worker.service".to_string(),
            kind: "service".to_string(),
            label: "worker.service".to_string(),
            state: "active".to_string(),
            first_seen_ns: 1,
            last_seen_ns: 1,
            valid_from_ns: 1,
            valid_to_ns: None,
            metadata_json: "{}".to_string(),
        })
        .expect("node");
    store
        .upsert_edge(&twin_store::EdgeRow {
            id: "service:worker.service|depends_on|service:postgresql.service".to_string(),
            from_node_id: "service:worker.service".to_string(),
            to_node_id: "service:postgresql.service".to_string(),
            kind: "depends_on".to_string(),
            class: "inferred".to_string(),
            state: "active".to_string(),
            evidence_score: 0,
            evidence_label: "weak".to_string(),
            evidence_count: 0,
            first_seen_ns: 1,
            last_seen_ns: 1,
            metadata_json: "{}".to_string(),
        })
        .expect("edge");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("postgresql".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert_eq!(result.direct_dependents.len(), 2);
    assert_eq!(result.risk.level, RiskLevel::High);
    assert!(result.unknowns.iter().any(|u| u.kind == "missing_evidence"));
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

fn write_systemd_fixture(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).expect("systemd dir");
    std::fs::write(
        dir.join("docker.service"),
        "[Unit]\nRequires=containerd.service\n",
    )
    .expect("docker.service");
    std::fs::write(
        dir.join("containerd.service"),
        "[Unit]\nDescription=containerd\n",
    )
    .expect("containerd.service");
}

fn scan_with_systemd(
    home: &IsolatedHome,
    proc: &std::path::Path,
    systemd: &std::path::Path,
) -> twin_app::ScanResult {
    let _env = support::lock_scan_env();
    support::set_systemd_unit_root(systemd);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), proc).expect("scan");
    support::clear_scan_env();
    result
}

#[test]
fn scan_in_declared_systemd_depends_on_edge() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-systemd-empty");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home.layout.data_dir.join("fixture-systemd");
    write_systemd_fixture(&systemd);
    let result = scan_with_systemd(&home, &proc, &systemd);
    assert!(result.declared_depends_on_edge_count >= 1);
    assert!(result.systemd_unit_count >= 2);
    let store = Store::open(&home.layout.db_file()).expect("open");
    let edge = store
        .get_edge("service:docker.service|depends_on|service:containerd.service")
        .expect("edge")
        .expect("depends_on edge");
    assert_eq!(edge.class, "observed");
}

#[test]
fn impact_in_declared_dependent_from_unit_file() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-systemd-impact");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home.layout.data_dir.join("fixture-systemd-impact");
    write_systemd_fixture(&systemd);
    scan_with_systemd(&home, &proc, &systemd);
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("containerd.service".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(result.direct_dependents.is_empty());
    assert_eq!(result.configured_dependents.len(), 1);
    assert_eq!(result.configured_dependents[0].id, "service:docker.service");
    assert_eq!(result.configured_dependents[0].impact_kind, "configured");
    assert_eq!(result.risk.level, RiskLevel::Low);
    assert!(result.configured_dependents[0]
        .evidence
        .iter()
        .any(|e| e.statement.contains("Requires=containerd.service")));
}

#[test]
fn scan_in_preserves_observed_depends_on_after_tcp_inference() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-systemd-tcp");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home.layout.data_dir.join("fixture-systemd-tcp");
    std::fs::create_dir_all(&systemd).expect("systemd");
    std::fs::write(
        systemd.join("django.service"),
        "[Unit]\nRequires=postgresql.service\n",
    )
    .expect("django.service");
    std::fs::write(
        systemd.join("postgresql.service"),
        "[Unit]\nDescription=postgres\n",
    )
    .expect("postgresql.service");
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
    scan_with_systemd(&home, &proc, &systemd);
    let store = Store::open(&home.layout.db_file()).expect("open");
    let edge = store
        .get_edge("service:django.service|depends_on|service:postgresql.service")
        .expect("edge");
    let Some(edge) = edge else {
        panic!("expected django depends on postgres");
    };
    assert_eq!(edge.class, "observed");
}

#[test]
fn impact_in_degraded_scan_still_low_without_local_unknowns() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&twin_store::CollectorRunRow {
            id: None,
            collector: COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 60,
            error_message: None,
            metadata_json:
                r#"{"fd_permission_denied":55,"socket_unmapped":0,"active_socket_unmapped":0}"#
                    .to_string(),
        })
        .expect("run");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("nginx".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(result.direct_dependents.is_empty());
    assert_eq!(result.risk.level, RiskLevel::Low);
    assert!(result
        .unknowns
        .iter()
        .all(|u| !u.weakens_evidence || u.kind == "scan_health"));
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

fn fixture_proc_with_unix_dependency(home: &IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc-unix-deps");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        99,
        "99 (dbus-broker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/dbus-broker\0",
        None,
        Some("0::/system.slice/dbus-broker.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        201,
        "201 (pipewire) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/pipewire\0",
        None,
        Some("0::/system.slice/pipewire.service\n"),
    );
    write_unix_table(
        &proc,
        "\
Num       RefCount Protocol Flags    Type St Inode Path
 1: 00000001 00000000 00010000 0001 01  9001 /run/dbus/system_bus_socket
 2: 00000001 00000000 00000000 0001 03  9002 /run/dbus/system_bus_socket
",
    );
    write_socket_fd(&proc, 99, 3, 9001);
    write_socket_fd(&proc, 201, 5, 9002);
    proc
}

#[test]
fn scan_in_persists_unix_listener_and_infers_service_dependency() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_unix_dependency(&home);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert_eq!(result.unix_listener_count, 1);
    assert_eq!(result.unix_connection_count, 1);
    assert!(result.service_listens_on_unix_edge_count >= 1);
    assert!(result.service_connects_to_unix_edge_count >= 1);
    assert!(result.service_depends_on_unix_edge_count >= 1);

    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_node("unix:/run/dbus/system_bus_socket")
        .expect("get")
        .is_some());
    assert!(store
        .list_edges_by_kind("depends_on")
        .expect("edges")
        .iter()
        .any(|e| {
            e.from_node_id == "service:pipewire.service"
                && e.to_node_id == "service:dbus-broker.service"
        }));
}

#[test]
fn graph_in_unix_socket_neighborhood() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_unix_dependency(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let unix = NodeId::unix_socket("/run/dbus/system_bus_socket").expect("unix");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(unix),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::UnixSocket(view) = result else {
        panic!("expected unix socket view");
    };
    assert!(!view.service_listeners.is_empty());
    assert!(!view.service_callers.is_empty());
}

#[test]
fn impact_in_unix_socket_target_lists_callers() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_unix_dependency(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let unix = NodeId::unix_socket("/run/dbus/system_bus_socket").expect("unix");
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(unix),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(!result.direct_dependents.is_empty());
    assert!(result
        .direct_dependents
        .iter()
        .any(|d| d.id == "service:pipewire.service"));
    assert!(result.direct_dependents.iter().any(|d| {
        d.evidence.iter().any(|e| {
            e.statement
                .contains("connected on /run/dbus/system_bus_socket")
        })
    }));
}

fn write_socket_activation_fixture(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).expect("systemd dir");
    std::fs::write(
        dir.join("dbus.socket"),
        "[Unit]\nDescription=D-Bus Socket\n\n[Socket]\nListenStream=/run/dbus/system_bus_socket\nService=dbus.service\n",
    )
    .expect("dbus.socket");
    std::fs::write(dir.join("dbus.service"), "[Unit]\nDescription=D-Bus\n").expect("dbus.service");
}

#[test]
fn scan_in_persists_socket_activation_edge() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-socket-activation");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home
        .layout
        .data_dir
        .join("fixture-systemd-socket-activation");
    write_socket_activation_fixture(&systemd);
    let result = scan_with_systemd(&home, &proc, &systemd);
    assert!(result.socket_activation_edge_count >= 1);
    let store = Store::open(&home.layout.db_file()).expect("open");
    let edge = store
        .get_edge("service:dbus.socket|depends_on|service:dbus.service")
        .expect("edge")
        .expect("socket activation edge");
    assert_eq!(edge.class, "observed");
    assert!(edge.metadata_json.contains("socket_activation"));
}

#[test]
fn graph_in_socket_unit_shows_activation_target() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-graph-socket");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home.layout.data_dir.join("fixture-systemd-graph-socket");
    write_socket_activation_fixture(&systemd);
    scan_with_systemd(&home, &proc, &systemd);
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("dbus.socket")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Service(view) = result else {
        panic!("expected service view");
    };
    assert_eq!(view.service.id, "service:dbus.socket");
    assert_eq!(view.socket_activation.len(), 1);
    assert_eq!(view.socket_activation[0].id, "service:dbus.service");
    assert_eq!(view.socket_activation[0].tag.as_deref(), Some("activates"));
}

#[test]
fn scan_in_applies_cgroup_correction_from_dbus_fixture() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-cgroup-correct");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        501,
        "501 (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/worker\0",
        None,
        Some("0::/system.slice/wrong.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-cgroup-correct");
    std::fs::create_dir_all(&systemd).expect("systemd");
    let _env = support::lock_scan_env();
    support::set_systemd_unit_root(&systemd);
    // SAFETY: test-only env override for fixture isolation.
    unsafe {
        std::env::set_var(
            "TWIN_SYSTEMD_CGROUP_MAP",
            r#"{"/system.slice/wrong.service":"correct.service"}"#,
        );
    }
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    support::clear_scan_env();
    assert!(result.cgroup_correction_count >= 1);
    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_edge("service:correct.service|owns|process:pid:501")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:wrong.service|owns|process:pid:501")
        .expect("stale edge")
        .is_none());
}

fn write_multi_sample_proc(
    base: &std::path::Path,
    sample: u32,
    client_inode: u64,
    client_port: u16,
) {
    let proc = base.join(format!("sample-{sample}"));
    std::fs::create_dir_all(&proc).expect("proc sample");
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
    let client_port_hex = format!("{:04X}", client_port);
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:{client_port_hex} 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 {client_inode} 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    write_socket_fd(&proc, 8841, 12, client_inode);
}

#[test]
fn scan_in_multi_sample_merges_ephemeral_connections() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-multi-sample");
    std::fs::create_dir_all(&proc).expect("proc");
    write_multi_sample_proc(&proc, 1, 456, 50_122);
    let sample_two = proc.join("sample-2");
    std::fs::create_dir_all(&sample_two).expect("sample-2");
    write_proc_fixture_with_cgroup(
        &sample_two,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &sample_two,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        &sample_two,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&sample_two, 721, 8, 12345);
    let result = twin_app::scan_in(
        &home.layout,
        ScanRequest {
            samples: 2,
            interval_secs: 1,
            ..ScanRequest::default()
        },
        &proc,
    )
    .expect("scan");
    assert_eq!(result.samples_completed, 2);
    assert_eq!(result.edges_first_seen_by_sample.len(), 2);
    assert!(result.edges_first_seen_by_sample[0] > 0);
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
