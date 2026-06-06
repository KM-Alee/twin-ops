mod support;

use std::path::Path;

use twin_app::{EmulateActionRequest, EmulateRequest, GraphRequest, InitRequest, ScanRequest};
use twin_core::NodeId;
use twin_emulate::{DELETE_SAFETY_STATEMENT, SAFETY_STATEMENT};
use twin_store::Store;

use support::{clear_scan_env, lock_scan_env, set_host_root, set_systemd_unit_root, IsolatedHome};

fn write_nginx_systemd_fixture(dir: &Path) {
    std::fs::create_dir_all(dir).expect("systemd dir");
    std::fs::write(dir.join("nginx.service"), "[Unit]\nDescription=nginx\n")
        .expect("nginx.service");
}

fn write_nginx_host_root(host_root: &Path) {
    let conf = host_root.join("etc/nginx/nginx.conf");
    std::fs::create_dir_all(conf.parent().expect("parent")).expect("dirs");
    std::fs::write(&conf, "events {}\n").expect("nginx.conf");
}

fn scan_nginx_fixture(home: &IsolatedHome, proc: &Path, systemd: &Path, host_root: &Path) {
    set_systemd_unit_root(systemd);
    set_host_root(host_root);
    twin_app::scan_in(&home.layout, ScanRequest::default(), proc).expect("scan");
    clear_scan_env();
}

#[test]
fn scan_in_persists_service_config_file_edges() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-config");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-config");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let store = Store::open(&home.layout.db_file()).expect("open");
    let file_id = "file:/etc/nginx/nginx.conf";
    assert!(store.get_node(file_id).expect("node").is_some());
    let edge = store
        .get_edge("service:nginx.service|configured_by|file:/etc/nginx/nginx.conf")
        .expect("edge")
        .expect("configured_by edge");
    assert_eq!(edge.class, "observed");
    let obs = store
        .list_observations_for_edge(&edge.id)
        .expect("obs")
        .len();
    assert!(obs >= 1);
}

#[test]
fn emulate_delete_file_reports_runtime_restart_and_persistent_impacts() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-emulate-delete");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-emulate-delete");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root-delete");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let result = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "/etc/nginx/nginx.conf".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate delete");
    assert_eq!(result.action, "delete");
    assert!(!result.action_performed);
    assert_eq!(result.safety_statement, DELETE_SAFETY_STATEMENT);
    assert_eq!(result.general_safety_statement, SAFETY_STATEMENT);
    assert_eq!(result.runtime_impacts.len(), 1);
    assert_eq!(result.restart_impacts.len(), 1);
    assert!(!result.persistent_impacts.is_empty());
}

#[test]
fn emulate_delete_file_does_not_mutate_file_or_graph() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-immutability");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-immutability");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root-immutability");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let conf = host_root.join("etc/nginx/nginx.conf");
    let before = std::fs::read_to_string(&conf).expect("read conf");
    let store = Store::open(&home.layout.db_file()).expect("open");
    let edges_before = store.count_edges().expect("edges");

    twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "file:/etc/nginx/nginx.conf".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");

    let after = std::fs::read_to_string(&conf).expect("read conf after");
    assert_eq!(before, after);
    let store = Store::open(&home.layout.db_file()).expect("open again");
    assert_eq!(store.count_edges().expect("edges after"), edges_before);
}

#[test]
fn graph_in_service_shows_configured_files() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-graph-service");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-graph-service");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root-graph-service");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("nginx.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph service");
    let twin_app::GraphResult::Service(service) = result else {
        panic!("expected service graph");
    };
    assert!(service
        .configured_files
        .iter()
        .any(|f| f.id == "file:/etc/nginx/nginx.conf"));
}

#[test]
fn graph_in_file_shows_configured_services() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-graph-file");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-graph-file");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root-graph-file");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::file("/etc/nginx/nginx.conf")),
            ..GraphRequest::default()
        },
    )
    .expect("graph file");
    let twin_app::GraphResult::File(file) = result else {
        panic!("expected file graph");
    };
    assert_eq!(file.configures.len(), 1);
    assert_eq!(file.configures[0].id, "service:nginx.service");
}

#[test]
fn emulate_delete_file_unknown_when_target_not_in_graph() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-empty");
    std::fs::create_dir_all(&proc).expect("proc");
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");

    let result = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "/etc/nginx/nginx.conf".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert!(!result.unknown_impacts.is_empty());
    assert!(result.unknowns.iter().any(|u| u.weakens_evidence));
}

#[test]
fn emulate_delete_rejects_relative_path() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "relative/path.conf".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect_err("relative path");
    assert!(
        err.to_string().contains("relative"),
        "error should mention relative path: {err}"
    );
}

#[test]
fn emulate_delete_rejects_service_target() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "service:nginx.service".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect_err("service target");
    assert!(
        err.to_string().contains("file"),
        "error should mention file requirement: {err}"
    );
}

#[test]
fn scan_in_persists_drop_in_config_file_edges() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-dropin");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-dropin");
    std::fs::create_dir_all(&systemd).expect("systemd dir");
    std::fs::write(systemd.join("nginx.service"), "[Unit]\nDescription=nginx\n")
        .expect("nginx.service");
    let dropin_dir = systemd.join("nginx.service.d");
    std::fs::create_dir_all(&dropin_dir).expect("dropin dir");
    std::fs::write(
        dropin_dir.join("override.conf"),
        "[Service]\nType=forking\n",
    )
    .expect("dropin");
    let host_root = home.layout.data_dir.join("fixture-host-root-dropin");
    std::fs::create_dir_all(&host_root).expect("host root");
    set_systemd_unit_root(&systemd);
    set_host_root(&host_root);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();

    let store = Store::open(&home.layout.db_file()).expect("open");
    let dropin_path = dropin_dir.join("override.conf");
    let file_id = format!("file:{}", dropin_path.display());
    assert!(
        store.get_node(&file_id).expect("node").is_some(),
        "drop-in file node should exist: {file_id}"
    );
    let edge_id = format!(
        "service:nginx.service|configured_by|file:{}",
        dropin_path.display()
    );
    let edge = store
        .get_edge(&edge_id)
        .expect("edge")
        .expect("configured_by edge for drop-in");
    assert_eq!(edge.class, "observed");
}

#[test]
fn graph_in_file_absolute_path_shorthand() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-graph-abs");
    std::fs::create_dir_all(&proc).expect("proc");
    support::write_proc_fixture_with_cgroup(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let systemd = home.layout.data_dir.join("fixture-systemd-graph-abs");
    write_nginx_systemd_fixture(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-root-graph-abs");
    write_nginx_host_root(&host_root);
    scan_nginx_fixture(&home, &proc, &systemd, &host_root);

    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::file("/etc/nginx/nginx.conf")),
            ..GraphRequest::default()
        },
    )
    .expect("graph file by absolute path");
    let twin_app::GraphResult::File(file) = result else {
        panic!("expected file graph result");
    };
    assert_eq!(file.configures.len(), 1);
    assert_eq!(file.configures[0].id, "service:nginx.service");
}
