mod support;

use std::path::PathBuf;
use std::str::FromStr;

use twin_app::{EmulateActionRequest, EmulateRequest, InitRequest, ScanRequest};
use twin_core::{DependentImpactKind, NodeId, RiskLevel, UnknownKind};
use twin_emulate::SAFETY_STATEMENT;
use twin_store::Store;

use support::{
    write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, write_unix_table,
    IsolatedHome,
};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

fn fixture_proc_with_active_connection(home: &IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc-emulate");
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

fn snapshot_target_edges(store: &Store, target: &str) -> Vec<(String, String, String)> {
    store
        .list_edges_to(target)
        .expect("edges")
        .into_iter()
        .map(|e| (e.id, e.from_node_id, e.state))
        .collect()
}

#[test]
fn emulate_restart_service_reports_runtime_dependent() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::emulate_in(&home.layout, EmulateRequest::restart_query("postgresql"))
        .expect("emulate");
    assert!(!result.action_performed);
    assert_eq!(result.safety_statement, SAFETY_STATEMENT);
    assert!(!result.transient_impacts.is_empty());
    assert!(result
        .transient_impacts
        .iter()
        .any(|i| i.id == "service:django.service"));
    assert_eq!(result.risk.level, RiskLevel::Medium);
}

#[test]
fn emulate_restart_service_marks_owned_tcp_port_unavailable() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::service("postgresql.service")),
                target_query: None,
                show_paths: false,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert!(result
        .overlay
        .unavailable_nodes
        .iter()
        .any(|n| { n.id == "port:tcp:127.0.0.1:5432" }));
}

#[test]
fn emulate_restart_rejects_port_target() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let err = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::port_tcp("127.0.0.1", 5432).expect("port")),
                target_query: None,
                show_paths: false,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect_err("port target");
    assert!(err.to_string().contains("unsupported"));
}

#[test]
fn emulate_restart_does_not_modify_graph() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let before_nodes = store.count_nodes().expect("nodes");
    let before_edges = store.count_edges().expect("edges");
    let before_target_edges = snapshot_target_edges(&store, "service:postgresql.service");
    let before_node = store
        .get_node("service:postgresql.service")
        .expect("get")
        .expect("row");

    twin_app::emulate_in(&home.layout, EmulateRequest::restart_query("postgresql"))
        .expect("emulate");

    let after_nodes = store.count_nodes().expect("nodes");
    let after_edges = store.count_edges().expect("edges");
    let after_target_edges = snapshot_target_edges(&store, "service:postgresql.service");
    let after_node = store
        .get_node("service:postgresql.service")
        .expect("get")
        .expect("row");
    assert_eq!(before_nodes, after_nodes);
    assert_eq!(before_edges, after_edges);
    assert_eq!(before_target_edges, after_target_edges);
    assert_eq!(before_node.state, after_node.state);
}

#[test]
fn emulate_restart_resolves_service_shorthand() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::emulate_in(
        &home.layout,
        EmulateRequest::restart_query("postgresql.service"),
    )
    .expect("emulate");
    assert_eq!(result.target, "service:postgresql.service");
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

fn scan_with_systemd(home: &IsolatedHome, proc: &std::path::Path, systemd: &std::path::Path) {
    let _env = support::lock_scan_env();
    support::set_systemd_unit_root(systemd);
    twin_app::scan_in(&home.layout, ScanRequest::default(), proc).expect("scan");
    support::clear_scan_env();
}

fn fixture_proc_with_unix_listener(home: &IsolatedHome) -> PathBuf {
    let proc = home.layout.data_dir.join("fixture-proc-emulate-unix");
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
    write_unix_table(
        &proc,
        "\
Num       RefCount Protocol Flags    Type St Inode Path
 1: 00000001 00000000 00010000 0001 01  9001 /run/dbus/system_bus_socket
",
    );
    write_socket_fd(&proc, 99, 3, 9001);
    proc
}

#[test]
fn emulate_restart_service_reports_configured_context() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-emulate-systemd");
    std::fs::create_dir_all(&proc).expect("proc");
    let systemd = home.layout.data_dir.join("fixture-systemd-emulate");
    write_systemd_fixture(&systemd);
    scan_with_systemd(&home, &proc, &systemd);
    let result = twin_app::emulate_in(
        &home.layout,
        EmulateRequest::restart_query("containerd.service"),
    )
    .expect("emulate");
    assert!(result.transient_impacts.is_empty());
    assert_eq!(result.configured_impacts.len(), 1);
    assert_eq!(result.configured_impacts[0].id, "service:docker.service");
    assert_eq!(result.risk.level, RiskLevel::Low);
}

#[test]
fn emulate_restart_service_marks_owned_unix_socket_unavailable() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_unix_listener(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::emulate_in(&home.layout, EmulateRequest::restart_query("dbus-broker"))
        .expect("emulate");
    assert!(result
        .overlay
        .unavailable_nodes
        .iter()
        .any(|n| { n.id.starts_with("unix:/run/dbus/system_bus_socket") }));
}

#[test]
fn emulate_restart_unknowns_are_target_scoped() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = fixture_proc_with_active_connection(&home);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    let result = twin_app::emulate_in(&home.layout, EmulateRequest::restart_query("postgresql"))
        .expect("emulate");
    assert!(result
        .unknowns
        .iter()
        .all(|u| u.source.as_deref() != Some("port:tcp:0.0.0.0:80")));
    assert!(result
        .transient_impacts
        .iter()
        .any(|i| i.id == "service:django.service"));
}

#[test]
fn emulate_restart_ambiguous_service_errors() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-emulate-ambiguous");
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
    let err = twin_app::emulate_in(&home.layout, EmulateRequest::restart_query("ng"))
        .expect_err("ambiguous");
    assert!(err.to_string().contains("ambiguous"));
}

#[test]
fn dependent_impact_kind_roundtrip() {
    assert_eq!(DependentImpactKind::Runtime.as_str(), "runtime");
    assert_eq!(
        DependentImpactKind::from_str("configured").unwrap(),
        DependentImpactKind::Configured
    );
    assert_eq!(UnknownKind::ScanHealth.as_str(), "scan_health");
}
