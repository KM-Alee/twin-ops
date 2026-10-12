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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan1");
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan2");
}

#[test]
fn scan_in_repeated_scan_updates_last_seen() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan1");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let first = store.get_node("process:pid:42").expect("get").expect("row");
    std::thread::sleep(std::time::Duration::from_millis(2));
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan2");
    let second = store.get_node("process:pid:42").expect("get").expect("row");
    assert_eq!(second.first_seen_ns, first.first_seen_ns);
    assert!(second.last_seen_ns >= first.last_seen_ns);
}

#[test]
fn graph_in_lists_process_nodes() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
    assert_eq!(result.tcp_listener_count, 1);
    assert_eq!(result.port_count, 1);
    assert!(
        result.socket_owner_inode_count >= 1,
        "socket_owner_inode_count=0; probe had owners"
    );
    assert!(
        result.process_listens_on_edge_count >= 1,
        "inet_listeners={} ports={} unmapped={} owner_inodes={} warnings={:?}",
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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

fn seed_node(store: &mut Store, id: &str, kind: &str, label: &str, state: &str) {
    store
        .upsert_node(&twin_store::NodeRow {
            id: id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            state: state.to_string(),
            first_seen_ns: 1,
            last_seen_ns: 1,
            valid_from_ns: 1,
            valid_to_ns: None,
            metadata_json: "{}".to_string(),
        })
        .expect("seed node");
}

fn listed_ids(home: &IsolatedHome, kind: NodeKind) -> Vec<String> {
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(kind),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::List(list) = result else {
        panic!("expected list");
    };
    assert_eq!(list.kind, kind);
    list.nodes.into_iter().map(|node| node.id).collect()
}

#[test]
fn graph_in_lists_file_unix_socket_and_cgroup_nodes_and_hides_gone() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    seed_node(
        &mut store,
        "file:/etc/nginx/nginx.conf",
        "file",
        "/etc/nginx/nginx.conf",
        "active",
    );
    seed_node(
        &mut store,
        "file:/etc/hosts",
        "file",
        "/etc/hosts",
        "active",
    );
    seed_node(
        &mut store,
        "file:/etc/gone.conf",
        "file",
        "/etc/gone.conf",
        "gone",
    );
    seed_node(
        &mut store,
        "unix:/run/app.sock",
        "unix_socket",
        "/run/app.sock",
        "active",
    );
    seed_node(
        &mut store,
        "cgroup:/system.slice/app.service",
        "cgroup",
        "/system.slice/app.service",
        "active",
    );
    drop(store);

    assert_eq!(
        listed_ids(&home, NodeKind::File),
        vec![
            "file:/etc/hosts".to_string(),
            "file:/etc/nginx/nginx.conf".to_string()
        ]
    );
    assert_eq!(
        listed_ids(&home, NodeKind::UnixSocket),
        vec!["unix:/run/app.sock".to_string()]
    );
    assert_eq!(
        listed_ids(&home, NodeKind::Cgroup),
        vec!["cgroup:/system.slice/app.service".to_string()]
    );
}

#[test]
fn graph_in_unsupported_kind_lists_supported_kinds() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let error = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            kind: Some(NodeKind::Host),
            ..GraphRequest::default()
        },
    )
    .expect_err("unsupported kind");
    let message = error.to_string();
    assert!(
        message.contains("unsupported graph kind: host"),
        "{message}"
    );
    assert!(message.contains("supported kinds"), "{message}");
    assert!(message.contains("file"), "{message}");
    assert!(message.contains("unix_socket"), "{message}");
}

#[test]
fn graph_in_service_shows_listening_ports() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");

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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    assert_eq!(result.risk.level, RiskLevel::Medium);
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(home), proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
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

#[test]
fn port_shorthand_resolves_unique_endpoints_and_rejects_ambiguity() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let now = twin_core::TimestampNs::now();
    store
        .upsert_node_typed(
            &twin_core::GraphNode::tcp_port("127.0.0.1", 5432, now, None).expect("port"),
        )
        .expect("insert");
    let request = GraphRequest {
        target_query: Some(":5432".into()),
        ..GraphRequest::default()
    };
    let graph = twin_app::graph_in(&home.layout, request.clone()).expect("unique");
    assert!(matches!(graph, twin_app::GraphResult::Port(_)));
    store
        .upsert_node_typed(&twin_core::GraphNode::tcp_port("::1", 5432, now, None).expect("port"))
        .expect("insert");
    let error = twin_app::graph_in(&home.layout, request).expect_err("ambiguous");
    assert!(error.to_string().contains("port:tcp:[::1]:5432"));
    assert!(error.to_string().contains("port:tcp:127.0.0.1:5432"));
    for query in [":", ":0", ":65536", ":wrong"] {
        assert!(twin_app::parse_port_query(query).is_err());
    }
}

#[test]
fn udp_scan_graph_impact_and_outage_preserve_protocol_separation() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    let tcp = std::fs::read_to_string(proc.join("net/tcp")).expect("tcp");
    let udp = tcp
        .replace(" 0A ", " 07 ")
        .replace(" 12345 ", " 12346 ")
        .replace(" 456 ", " 457 ");
    write_socket_fd(&proc, 721, 9, 12346);
    write_socket_fd(&proc, 8841, 13, 457);
    write_tcp_table(&proc, "udp", &udp);
    write_tcp_table(&proc, "udp6", TCP_HEADER);
    let result =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
    assert_eq!(result.tcp_listener_count, 1);
    assert_eq!(result.udp_listener_count, 1);
    assert_eq!(result.tcp_connection_count, 1);
    assert_eq!(result.udp_connection_count, 1);
    let store = Store::open(&home.layout.db_file()).expect("store");
    let udp_id = "port:udp:127.0.0.1:5432";
    assert!(store.get_node(udp_id).expect("UDP").is_some());
    assert!(store
        .get_node("port:tcp:127.0.0.1:5432")
        .expect("TCP")
        .is_some());
    let edge_id = format!("service:django.service|connects_to|{udp_id}");
    let links = store.list_observations_for_edge(&edge_id).expect("links");
    assert!(!links.is_empty());
    let graph = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(udp_id.parse().expect("UDP ID")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let encoded = serde_json::to_string(&graph).expect("graph JSON");
    assert!(encoded.contains(udp_id));
    let impact = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(udp_id.parse().expect("UDP ID")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(impact
        .direct_dependents
        .iter()
        .any(|peer| peer.id == "service:django.service"));
    assert!(impact
        .direct_dependents
        .iter()
        .flat_map(|peer| &peer.evidence)
        .any(|line| line.source.starts_with("/proc/net/udp:")));
    std::fs::remove_file(proc.join("net/udp")).expect("outage");
    let result = twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc)
        .expect("outage scan");
    assert!(result
        .coverage
        .unavailable_collectors
        .iter()
        .any(|source| source == "proc_net_udp"));
    assert_eq!(
        store.get_node(udp_id).expect("UDP").expect("node").state,
        "active"
    );
    assert_eq!(
        store.get_edge(&edge_id).expect("edge").expect("edge").state,
        "active"
    );
    write_tcp_table(&proc, "udp", TCP_HEADER);
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc)
        .expect("empty UDP scan");
    assert_eq!(
        store.get_node(udp_id).expect("UDP").expect("node").state,
        "gone"
    );
    assert_eq!(
        store
            .get_node("port:tcp:127.0.0.1:5432")
            .expect("TCP")
            .expect("node")
            .state,
        "active"
    );
}

#[test]
fn observed_cgroup_instance_loads_template_without_explicit_instance_file() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("proc-template-instance");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(&proc, 501,
        "501 (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid: 1000 1000 1000 1000\n", b"worker\0", None, Some("0::/system.slice/worker@blue.service\n"));
    let systemd = home.layout.data_dir.join("systemd-template-instance");
    std::fs::create_dir_all(&systemd).expect("root");
    std::fs::write(
        systemd.join("worker@.service"),
        "[Unit]\nRequires=database@%i.service\n",
    )
    .expect("template");
    std::fs::write(
        systemd.join("api.socket"),
        "[Socket]\nListenStream=vsock:3:8080\nAccept=yes\n",
    )
    .expect("socket");
    scan_with_systemd(&home, &proc, &systemd);
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert!(store
        .get_edge("service:worker@blue.service|depends_on|service:database@blue.service")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:api.socket|depends_on|service:api@.service")
        .expect("activation")
        .is_some());
    let template = store
        .get_node("service:api@.service")
        .expect("template")
        .expect("template node");
    assert!(template.metadata_json.contains("service_template"));
    assert!(!template.metadata_json.contains("active_state"));
}

#[test]
fn reused_pid_resets_lifetime_and_closes_previous_runtime_edges() {
    let _env = support::lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc(&home);
    let empty = tempfile::TempDir::new().expect("host");
    support::set_host_root(empty.path());
    std::fs::create_dir_all(proc.join("sys/kernel/random")).expect("boot dir");
    std::fs::write(
        proc.join("sys/kernel/random/boot_id"),
        "11111111-1111-1111-1111-111111111111",
    )
    .expect("boot");
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("first");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let first = store
        .get_node_typed(&NodeId::process(42))
        .expect("query")
        .expect("process");
    let endpoint =
        twin_core::GraphNode::tcp_port("192.0.2.10", 443, twin_core::TimestampNs::now(), None)
            .expect("endpoint");
    store
        .upsert_node(&twin_store::NodeRow::from(&endpoint))
        .expect("endpoint");
    let mut edge = twin_store::EdgeRow::from(&twin_core::GraphEdge::observed_process_connects_to(
        first.id(),
        endpoint.id(),
        twin_core::TimestampNs::now(),
        None,
    ));
    edge.metadata_json = r#"{"source":"ebpf_tcp"}"#.into();
    store.upsert_edge(&edge).expect("edge");
    let stat = std::fs::read_to_string(proc.join("42/stat")).expect("stat");
    let close = stat.rfind(')').expect("comm");
    let mut fields: Vec<_> = stat[close + 1..]
        .split_whitespace()
        .map(str::to_string)
        .collect();
    fields[19] = "9999".into();
    std::fs::write(
        proc.join("42/stat"),
        format!("{} {}", &stat[..=close], fields.join(" ")),
    )
    .expect("new stat");
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc)
        .expect("replacement");
    support::clear_scan_env();
    let replacement = store
        .get_node_typed(&NodeId::process(42))
        .expect("query")
        .expect("process");
    assert!(replacement.first_seen() > first.first_seen());
    assert!(replacement.metadata().as_str().contains("9999"));
    assert_eq!(
        store.get_edge(&edge.id).expect("edge").expect("row").state,
        "gone"
    );
}

#[test]
fn missing_tcp_table_preserves_previous_listener_as_uncertain() {
    let _env = support::lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_listener(&home);
    let empty = tempfile::TempDir::new().expect("host");
    support::set_host_root(empty.path());
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("first");
    std::fs::remove_file(proc.join("net/tcp")).expect("missing table");
    twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("partial");
    support::clear_scan_env();
    let store = Store::open(&home.layout.db_file()).expect("store");
    let edges = store.list_edges_from("process:pid:721").expect("edges");
    assert!(edges
        .iter()
        .any(|edge| edge.kind == "listens_on" && edge.state == "stale"));
}

#[test]
fn host_wide_scan_warnings_are_grouped_once_per_detail_in_impact_unknowns() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("grouped-warning-proc");
    std::fs::create_dir_all(&proc).expect("proc");
    for pid in 100..112u32 {
        write_proc_fixture_with_cgroup(
            &proc,
            pid,
            &format!("{pid} (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
            "Uid:\t0\t0\t0\t0\n",
            b"worker\0",
            None,
            Some("0::/system.slice/api.service\n"),
        );
    }
    let scan =
        twin_app::scan_in(&home.layout, support::host_isolated_scan(&home), &proc).expect("scan");
    let repeated = scan
        .warning_details
        .iter()
        .filter(|warning| warning.detail == scan.warning_details[0].detail)
        .count();
    assert!(
        repeated > 1,
        "fixture must repeat one warning: {:?}",
        scan.warning_details
    );
    let result = twin_app::impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("api".to_string()),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    let matching = result
        .unknowns
        .iter()
        .filter(|unknown| unknown.detail == scan.warning_details[0].detail)
        .collect::<Vec<_>>();
    assert_eq!(matching.len(), 1, "unknowns: {:?}", result.unknowns);
    assert!(matching[0]
        .source
        .as_deref()
        .is_some_and(|source| source.ends_with(&format!("(+{} similar)", repeated - 1))));
}
