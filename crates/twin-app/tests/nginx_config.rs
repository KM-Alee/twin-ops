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
    twin_app::scan_in_with_dns(&home.layout, ScanRequest::default(), proc, &FixtureDns)
        .expect("scan");
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
        changed
            .changed_nodes
            .iter()
            .any(|n| n.id == "file:/etc/nginx/nginx.conf"),
        "changed nodes: {:?}",
        changed.changed_nodes
    );
}

#[test]
fn nginx_includes_follow_globs_and_cycles_with_file_evidence_and_secret_safe_origins() {
    let _env = lock_scan_env();
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("proc-includes");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("units-includes");
    write_units(&systemd);
    let host = home.layout.data_dir.join("host-includes");
    write_conf(
        &host,
        "http { include conf.d/*.conf; include missing.conf; }\n",
    );
    std::fs::create_dir_all(host.join("etc/nginx/conf.d")).expect("dirs");
    let content = "include nginx.conf; proxy_pass http://user:PRIVATE_PASSWORD@127.0.0.1:8000/private?token=PRIVATE_TOKEN;";
    std::fs::write(host.join("etc/nginx/conf.d/api.conf"), content).expect("include");
    set_systemd_unit_root(&systemd);
    set_host_root(&host);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();
    let report = serde_json::to_string(&result).expect("json");
    assert!(report.contains("included file is missing"), "{report}");
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert!(store
        .get_edge("service:nginx.service|configured_by|file:/etc/nginx/conf.d/api.conf")
        .expect("config")
        .is_some());
    assert!(store
        .get_edge("service:nginx.service|proxies_to|service:django.service")
        .expect("proxy")
        .is_some());
    let observations = store
        .list_observations_by_kind("config_proxy_pass")
        .expect("observations");
    assert!(!observations.is_empty());
    let stored = format!("{observations:?}");
    assert!(!stored.contains("PRIVATE_PASSWORD"));
    assert!(!stored.contains("PRIVATE_TOKEN"));
    assert_eq!(
        std::fs::read_to_string(host.join("etc/nginx/conf.d/api.conf")).expect("file"),
        content
    );
}

#[test]
fn named_upstream_from_include_resolves_with_both_file_sources() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("proc-upstream");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("units-upstream");
    write_units(&systemd);
    let host = home.layout.data_dir.join("host-upstream");
    write_conf(&host, "http { include conf.d/*.conf; server { location / { proxy_pass http://api_pool/private?token=SECRET; } } }");
    std::fs::create_dir_all(host.join("etc/nginx/conf.d")).expect("dirs");
    std::fs::write(
        host.join("etc/nginx/conf.d/upstream.conf"),
        "upstream api_pool { server 127.0.0.1:8000; server 192.0.2.1:9000 down; }",
    )
    .expect("upstream");
    set_systemd_unit_root(&systemd);
    set_host_root(&host);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();
    let store = Store::open(&home.layout.db_file()).expect("store");
    let edge = store
        .get_edge("service:nginx.service|proxies_to|service:django.service")
        .expect("proxy")
        .expect("edge");
    assert_eq!(edge.class, "inferred");
    for file in ["/etc/nginx/nginx.conf", "/etc/nginx/conf.d/upstream.conf"] {
        assert!(store
            .get_edge(&format!("file:{file}|references|port:tcp:127.0.0.1:8000"))
            .expect("reference")
            .is_some());
    }
    assert!(store
        .get_node("port:tcp:192.0.2.1:9000")
        .expect("down")
        .is_none());
    let observations = store
        .list_observations_by_kind("config_proxy_pass")
        .expect("observations");
    assert!(observations
        .iter()
        .any(
            |obs| serde_json::from_str::<serde_json::Value>(&obs.metadata_json)
                .ok()
                .as_ref()
                .and_then(|value| value.get("__raw_ref"))
                .and_then(|value| value.as_str())
                == Some("file:/etc/nginx/nginx.conf")
        ));
    assert!(observations
        .iter()
        .any(
            |obs| serde_json::from_str::<serde_json::Value>(&obs.metadata_json)
                .ok()
                .as_ref()
                .and_then(|value| value.get("__raw_ref"))
                .and_then(|value| value.as_str())
                == Some("file:/etc/nginx/conf.d/upstream.conf")
        ));
    assert!(!format!("{observations:?}").contains("SECRET"));
    // Conflicting declarations must not choose a backend arbitrarily.
    std::fs::write(
        host.join("etc/nginx/conf.d/duplicate.conf"),
        "upstream api_pool { server 127.0.0.1:9999; }",
    )
    .expect("duplicate");
    set_systemd_unit_root(&systemd);
    set_host_root(&host);
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();
    assert!(result
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("ambiguous")));
    assert!(store
        .get_node("port:tcp:127.0.0.1:9999")
        .expect("ambiguous target")
        .is_none());
}

#[test]
fn incomplete_config_marks_proxy_evidence_stale_then_complete_scans_finalize_or_reactivate() {
    let _env = lock_scan_env();
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("nginx-lifecycle-proc");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("nginx-lifecycle-systemd");
    write_units(&systemd);
    let host = home.layout.data_dir.join("nginx-lifecycle-host");
    let valid = "http { server { location / { proxy_pass http://127.0.0.1:8000; } location /remote { proxy_pass http://192.0.2.1:9000; } } }";
    write_conf(&host, valid);
    scan_fixture(&home, &proc, &systemd, &host);
    let store = Store::open(&home.layout.db_file()).unwrap();
    let proxy = "service:nginx.service|proxies_to|service:django.service";
    let reference = "file:/etc/nginx/nginx.conf|references|port:tcp:192.0.2.1:9000";
    let before = store.get_edge(proxy).unwrap().unwrap().last_seen_ns;
    for _ in 0..2 {
        write_conf(&host, "http { server { proxy_pass");
        scan_fixture(&home, &proc, &systemd, &host);
        let edge = store.get_edge(proxy).unwrap().unwrap();
        assert_eq!(edge.state, "stale");
        assert_eq!(
            edge.last_seen_ns, before,
            "missing evidence must not refresh last_seen"
        );
        assert_eq!(store.get_edge(reference).unwrap().unwrap().state, "stale");
        assert_eq!(
            store
                .get_node("port:tcp:192.0.2.1:9000")
                .unwrap()
                .unwrap()
                .state,
            "stale"
        );
        assert_eq!(
            store
                .get_node("port:tcp:127.0.0.1:8000")
                .unwrap()
                .unwrap()
                .state,
            "active",
            "live socket evidence remains current"
        );
    }
    write_conf(&host, "events {} http {}");
    set_systemd_unit_root(&systemd);
    set_host_root(&host);
    let complete = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).unwrap();
    clear_scan_env();
    assert_eq!(
        store.get_edge(proxy).unwrap().unwrap().state,
        "gone",
        "{:?}",
        complete.warning_details
    );
    assert_eq!(store.get_edge(reference).unwrap().unwrap().state, "gone");
    assert_eq!(
        store
            .get_node("port:tcp:192.0.2.1:9000")
            .unwrap()
            .unwrap()
            .state,
        "gone"
    );
    write_conf(&host, valid);
    scan_fixture(&home, &proc, &systemd, &host);
    assert_eq!(store.get_edge(proxy).unwrap().unwrap().state, "active");
    assert_eq!(store.get_edge(reference).unwrap().unwrap().state, "active");
    assert_eq!(
        store
            .get_node("port:tcp:192.0.2.1:9000")
            .unwrap()
            .unwrap()
            .state,
        "active"
    );
}

#[test]
fn named_upstream_matching_is_case_insensitive_and_explicit_ports_are_rejected() {
    let _env = lock_scan_env();
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("upstream-port-proc");
    nginx_and_django_proc(&proc);
    let systemd = home.layout.data_dir.join("upstream-port-systemd");
    write_units(&systemd);
    let host = home.layout.data_dir.join("upstream-port-host");
    write_conf(
        &host,
        "http { upstream Pool { server 127.0.0.1:8000; } server { proxy_pass http://pOOL:80; } }",
    );
    set_systemd_unit_root(&systemd);
    set_host_root(&host);
    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).unwrap();
    clear_scan_env();
    assert!(scan
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("must not specify a port")));
    let store = Store::open(&home.layout.db_file()).unwrap();
    let proxy = "service:nginx.service|proxies_to|service:django.service";
    assert!(store.get_edge(proxy).unwrap().is_none());
    write_conf(
        &host,
        "http { upstream Pool { server 127.0.0.1:8000; } server { proxy_pass http://pOOL; } }",
    );
    scan_fixture(&home, &proc, &systemd, &host);
    assert_eq!(store.get_edge(proxy).unwrap().unwrap().state, "active");
}

#[test]
fn upstream_server_includes_inherit_the_enclosing_group() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("proc-context");
    nginx_and_django_proc(&proc);
    let units = home.layout.data_dir.join("units-context");
    write_units(&units);
    let host = home.layout.data_dir.join("host-context");
    write_conf(&host, "http { upstream backend { include servers.conf; } server { proxy_pass http://backend; } } stream { upstream backend { server 127.0.0.1:9000; } server { proxy_pass backend; } }");
    std::fs::write(
        host.join("etc/nginx/servers.conf"),
        "server 127.0.0.1:8000;",
    )
    .expect("servers");
    scan_fixture(&home, &proc, &units, &host);
    let store = Store::open(&home.layout.db_file()).expect("store");
    assert!(store
        .get_edge("service:nginx.service|proxies_to|service:django.service")
        .unwrap()
        .is_some());
    assert!(store.get_node("port:tcp:127.0.0.1:9000").unwrap().is_some());
}

#[test]
fn current_hostname_resolution_is_stored_as_inference_with_dns_provenance() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("proc-dns");
    nginx_and_django_proc(&proc);
    let units = home.layout.data_dir.join("units-dns");
    write_units(&units);
    let host = home.layout.data_dir.join("host-dns");
    write_conf(
        &host,
        "http { server { proxy_pass http://localhost:8000; } }",
    );
    scan_fixture(&home, &proc, &units, &host);
    let store = Store::open(&home.layout.db_file()).expect("store");
    let observations = store
        .list_observations_by_kind("dns_address_resolved")
        .unwrap();
    assert!(!observations.is_empty());
    assert!(observations
        .iter()
        .all(|observation| observation.source == "system_dns"));
    for observation in observations {
        let edge = store
            .get_edge(&format!(
                "file:/etc/nginx/nginx.conf|references|{}",
                observation.object_node_id.unwrap()
            ))
            .unwrap()
            .unwrap();
        assert_eq!(edge.class, "inferred");
    }
}

struct FixtureDns;
impl twin_collectors::dns::DnsReader for FixtureDns {
    fn resolve(
        &self,
        hosts: &std::collections::BTreeSet<String>,
    ) -> twin_collectors::dns::DnsAnswers {
        hosts
            .iter()
            .map(|host| {
                (
                    host.clone(),
                    if host == "localhost" {
                        Ok(vec![
                            std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST),
                            std::net::IpAddr::V6(std::net::Ipv6Addr::LOCALHOST),
                        ])
                    } else {
                        Err(twin_collectors::dns::DnsError::Unavailable)
                    },
                )
            })
            .collect()
    }
}

#[test]
fn configured_stream_udp_backend_does_not_create_a_tcp_reference() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("proc-udp-config");
    nginx_and_django_proc(&proc);
    let units = home.layout.data_dir.join("units-udp-config");
    write_units(&units);
    let host = home.layout.data_dir.join("host-udp-config");
    write_conf(
        &host,
        "stream { server { proxy_pass 127.0.0.1:53; listen 5300 udp; } }",
    );
    scan_fixture(&home, &proc, &units, &host);
    let store = Store::open(&home.layout.db_file()).unwrap();
    let observation = store
        .list_observations_by_kind("config_proxy_pass")
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    assert_eq!(
        observation.object_node_id.as_deref(),
        Some("port:udp:127.0.0.1:53")
    );
    assert!(store.get_node("port:tcp:127.0.0.1:53").unwrap().is_none());
}

#[test]
fn unix_proxy_joins_an_observed_listener_and_failed_dns_preserves_uncertainty() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("proc-unix-dns");
    nginx_and_django_proc(&proc);
    support::write_unix_table(&proc, "Num RefCount Protocol Flags Type St Inode Path\n1: 00000001 00000000 00010000 0001 01 9001 /run/api.sock\n");
    write_socket_fd(&proc, 721, 9, 9001);
    let units = home.layout.data_dir.join("units-unix-dns");
    write_units(&units);
    let host = home.layout.data_dir.join("host-unix-dns");
    write_conf(
        &host,
        "stream { server { proxy_pass unix:/run/api.sock; } }",
    );
    scan_fixture(&home, &proc, &units, &host);
    let store = Store::open(&home.layout.db_file()).unwrap();
    let proxy = "service:nginx.service|proxies_to|service:django.service";
    assert_eq!(store.get_edge(proxy).unwrap().unwrap().class, "inferred");
    write_conf(
        &host,
        "http { server { proxy_pass http://unavailable.example:8000; } }",
    );
    set_systemd_unit_root(&units);
    set_host_root(&host);
    let result =
        twin_app::scan_in_with_dns(&home.layout, ScanRequest::default(), &proc, &FixtureDns)
            .unwrap();
    clear_scan_env();
    assert!(result
        .warning_details
        .iter()
        .any(|warning| warning.detail.contains("hostname resolution unavailable")));
    assert_eq!(store.get_edge(proxy).unwrap().unwrap().state, "stale");
    assert!(store
        .list_observations_by_kind("dns_address_resolved")
        .unwrap()
        .is_empty());
}
