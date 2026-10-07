mod support;

use std::path::Path;

use twin_app::{
    EmulateActionRequest, EmulateRequest, GraphRequest, InitRequest, ScanRequest,
    WhatChangedRequest,
};
use twin_core::NodeId;
use twin_store::Store;

use support::{
    clear_scan_env, lock_scan_env, set_host_root, set_systemd_unit_root,
    write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, IsolatedHome,
};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

fn write_units(dir: &Path) {
    std::fs::create_dir_all(dir).expect("systemd dir");
    std::fs::write(dir.join("nginx.service"), "[Unit]\nDescription=nginx\n").expect("unit");
}

fn write_conf(host_root: &Path, body: &str) {
    let conf = host_root.join("etc/nginx/nginx.conf");
    std::fs::create_dir_all(conf.parent().expect("parent")).expect("dirs");
    std::fs::write(&conf, body).expect("nginx.conf");
}

fn nginx_and_django_proc(proc: &Path) {
    std::fs::create_dir_all(proc).expect("proc");
    write_proc_fixture_with_cgroup(
        proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    write_proc_fixture_with_cgroup(
        proc,
        721,
        "721 (django) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/django\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1F40 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(proc, 721, 8, 12345);
}

fn scan_fixture(home: &IsolatedHome, proc: &Path, systemd: &Path, host_root: &Path) {
    set_systemd_unit_root(systemd);
    set_host_root(host_root);
    twin_app::scan_in(&home.layout, ScanRequest::default(), proc).expect("scan");
    clear_scan_env();
}

#[test]
fn scan_infers_nginx_proxy_and_file_reference() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-nginx-proxy");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("fixture-systemd-nginx-proxy");
    write_units(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-nginx-proxy");
    write_conf(
        &host_root,
        "events {}\nhttp {\n  server {\n    location / {\n      proxy_pass http://127.0.0.1:8000;\n    }\n  }\n}\n",
    );
    scan_fixture(&home, &proc, &systemd, &host_root);

    let store = Store::open(&home.layout.db_file()).expect("open");
    let proxy = store
        .get_edge("service:nginx.service|proxies_to|service:django.service")
        .expect("edge")
        .expect("proxies_to");
    assert_eq!(proxy.class, "inferred");
    let reference = store
        .get_edge("file:/etc/nginx/nginx.conf|references|port:tcp:127.0.0.1:8000")
        .expect("edge")
        .expect("references");
    assert_eq!(reference.class, "observed");
    let file = store
        .get_node("file:/etc/nginx/nginx.conf")
        .expect("node")
        .expect("file node");
    assert!(file.metadata_json.contains("content_hash"));

    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("nginx.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Service(service) = result else {
        panic!("expected service graph");
    };
    assert!(service
        .dependencies
        .iter()
        .any(|d| { d.id == "service:django.service" && d.tag.as_deref() == Some("proxy") }));
    let evidence: Vec<_> = service
        .evidence
        .iter()
        .map(|l| l.statement.clone())
        .collect();
    assert!(
        service.evidence.iter().any(|line| {
            line.statement.contains("proxy_pass http://127.0.0.1:8000")
                && line.statement.contains("django.service")
        }),
        "evidence: {evidence:?}"
    );

    let file_view = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::file("/etc/nginx/nginx.conf")),
            ..GraphRequest::default()
        },
    )
    .expect("file graph");
    let twin_app::GraphResult::File(file_result) = file_view else {
        panic!("expected file graph");
    };
    assert!(file_result
        .references
        .iter()
        .any(|n| n.id == "port:tcp:127.0.0.1:8000"));

    let deleted = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::DeleteFile {
                path: "/etc/nginx/nginx.conf".to_string(),
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert!(deleted
        .evidence_lines
        .iter()
        .any(|line| { line.contains("proxy_pass to service:django.service") }));
}

#[test]
fn malformed_proxy_pass_warns_and_scan_succeeds() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-nginx-bad");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("fixture-systemd-nginx-bad");
    write_units(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-nginx-bad");
    write_conf(&host_root, "proxy_pass ;\n");
    set_systemd_unit_root(&systemd);
    set_host_root(&host_root);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();
    assert!(result
        .warning_details
        .iter()
        .any(|w| { w.kind == "config_parse" && w.detail.contains("proxy_pass") }));
    let store = Store::open(&home.layout.db_file()).expect("open");
    assert!(store
        .get_edge("service:nginx.service|proxies_to|service:django.service")
        .expect("edge")
        .is_none());
}

#[test]
fn config_byte_change_appears_in_what_changed() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-nginx-change");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("fixture-systemd-nginx-change");
    write_units(&systemd);
    let host_root = home.layout.data_dir.join("fixture-host-nginx-change");
    write_conf(&host_root, "proxy_pass http://127.0.0.1:8000;\n");
    scan_fixture(&home, &proc, &systemd, &host_root);
    write_conf(&host_root, "proxy_pass http://127.0.0.1:8000;\n# edited\n");
    scan_fixture(&home, &proc, &systemd, &host_root);

    let changed = twin_app::what_changed_in(
        &home.layout,
        WhatChangedRequest {
            since: "1h".to_string(),
            config_override: None,
            verbose: false,
        },
    )
    .expect("what-changed");
    assert!(
        changed
            .changed_nodes
            .iter()
            .any(|n| n.id == "file:/etc/nginx/nginx.conf"),
        "changed nodes: {:?}",
        changed.changed_nodes
    );
}
