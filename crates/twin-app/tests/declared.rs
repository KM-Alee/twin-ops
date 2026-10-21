mod support;

use twin_app::{impact_in, scan_in, ImpactRequest, InitRequest};
use twin_core::{EdgeId, EdgeKind, EdgeState, NodeId};
use twin_store::Store;

const SENTINEL: &str = "SENTINEL_DECLARED_SECRET";
const TCP: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:1F90 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 6001 1 0000000000000000 100 0 0 10 0";

struct DeclaredHost {
    home: support::IsolatedHome,
    proc: std::path::PathBuf,
    units: std::path::PathBuf,
    root: std::path::PathBuf,
}

impl DeclaredHost {
    fn new() -> Self {
        let home = support::IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
        let proc = home.layout.data_dir.join("proc");
        let units = home.layout.data_dir.join("units");
        let root = home.layout.data_dir.join("host");
        for directory in [
            &proc,
            &units,
            &root.join("etc/app"),
            &root.join("etc/haproxy"),
        ] {
            std::fs::create_dir_all(directory).expect("directory");
        }
        for unit in ["postgresql.service", "haproxy.service"] {
            std::fs::write(units.join(unit), "[Service]\n").expect("unit");
        }
        std::fs::write(
            units.join("app.service"),
            format!(
                "[Service]\nEnvironment=\"DATABASE_URL=postgres://app:{SENTINEL}@127.0.0.1:5432/app?password={SENTINEL}\" \"DB_PASSWORD={SENTINEL}\"\nEnvironmentFile=/etc/app/app.env\nEnvironmentFile=-/etc/app/optional.env\n"
            ),
        )
        .expect("app unit");
        std::fs::write(
            root.join("etc/app/app.env"),
            format!("API_TOKEN={SENTINEL}\nSEARCH_URL=http://u:{SENTINEL}@search.invalid:9200\n"),
        )
        .expect("env file");
        std::fs::write(
            root.join("etc/haproxy/haproxy.cfg"),
            format!(
                "userlist ops\n  user admin password {SENTINEL}\nbackend app\n  server app1 127.0.0.1:8080 check\n"
            ),
        )
        .expect("haproxy");
        for (pid, comm, unit) in [
            (721, "postgres", "postgresql.service"),
            (800, "app", "app.service"),
            (900, "haproxy", "haproxy.service"),
        ] {
            support::write_proc_fixture_with_cgroup(
                &proc,
                pid,
                &format!("{pid} ({comm}) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
                "Uid:\t0\t0\t0\t0\n",
                format!("/usr/bin/{comm}\0").as_bytes(),
                None,
                Some(&format!("0::/system.slice/{unit}\n")),
            );
        }
        support::write_tcp_table(&proc, "tcp", TCP);
        support::write_socket_fd(&proc, 721, 6, 12345);
        support::write_socket_fd(&proc, 800, 7, 6001);
        Self {
            home,
            proc,
            units,
            root,
        }
    }

    fn scan(&self) -> twin_app::ScanResult {
        let _env = support::lock_scan_env();
        support::set_systemd_unit_root(&self.units);
        support::set_host_root(&self.root);
        let result = scan_in(
            &self.home.layout,
            support::host_isolated_scan(&self.home),
            &self.proc,
        );
        support::clear_scan_env();
        result.expect("scan")
    }

    fn edge_state(&self, from: &str, kind: EdgeKind, to: &str) -> Option<EdgeState> {
        let store = Store::open(&self.home.layout.db_file()).expect("store");
        store
            .get_edge_typed(&EdgeId::new(
                &NodeId::service(from),
                kind,
                &NodeId::service(to),
            ))
            .expect("read")
            .map(|edge| edge.state())
    }
}

#[test]
fn an_environment_url_makes_its_service_a_dependent_of_the_listener() {
    let host = DeclaredHost::new();
    let scan = host.scan();
    assert!(scan.declared_dependency_count >= 2, "{scan:?}");
    let impact = impact_in(
        &host.home.layout,
        ImpactRequest {
            target: Some(NodeId::service("postgresql.service")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    let app = impact
        .direct_dependents
        .iter()
        .find(|dependent| dependent.id == "service:app.service")
        .expect("the declared client is a dependent");
    assert_eq!(app.impact_kind, "runtime");
    assert!(
        app.reason.starts_with("declared in ")
            && app
                .reason
                .ends_with("app.service (DATABASE_URL → postgres://127.0.0.1:5432)"),
        "{}",
        app.reason
    );
    assert_eq!(
        host.edge_state("haproxy.service", EdgeKind::ProxiesTo, "app.service"),
        Some(EdgeState::Active),
        "haproxy's backend is the app's listener"
    );
    let unresolved = scan.warning_details.iter().any(|detail| {
        detail
            .detail
            .contains("`search.invalid` could not be resolved")
    });
    assert!(unresolved, "{:?}", scan.warning_details);
}

#[test]
fn declared_values_never_reach_the_store_or_any_answer() {
    let host = DeclaredHost::new();
    let scan = serde_json::to_string(&host.scan()).expect("scan json");
    assert!(!scan.contains(SENTINEL), "scan result");
    let why = twin_app::why_in(
        &host.home.layout,
        twin_app::why_request("service:app.service", None).expect("request"),
    )
    .expect("why");
    let why = serde_json::to_string(&why).expect("why json");
    assert!(!why.contains(SENTINEL), "why");
    assert!(why.contains("postgres://127.0.0.1:5432"), "{why}");
    for file in std::fs::read_dir(&host.home.layout.data_dir)
        .expect("data dir")
        .flatten()
    {
        if file.path().to_string_lossy().contains("twin.db") {
            let bytes = std::fs::read(file.path()).expect("db bytes");
            assert!(
                !bytes
                    .windows(SENTINEL.len())
                    .any(|window| window == SENTINEL.as_bytes()),
                "{} holds the sentinel",
                file.path().display()
            );
        }
    }
}

#[test]
fn a_removed_declaration_is_gone_and_a_missing_env_file_keeps_it_stale() {
    let host = DeclaredHost::new();
    host.scan();
    std::fs::write(
        host.units.join("app.service"),
        "[Service]\nEnvironmentFile=/etc/app/app.env\n",
    )
    .expect("rewrite unit");
    host.scan();
    assert_eq!(
        host.edge_state("app.service", EdgeKind::DependsOn, "postgresql.service"),
        Some(EdgeState::Gone)
    );

    std::fs::write(
        host.root.join("etc/app/app.env"),
        "DATABASE_URL=postgres://localhost/app\n",
    )
    .expect("env");
    host.scan();
    assert_eq!(
        host.edge_state("app.service", EdgeKind::DependsOn, "postgresql.service"),
        Some(EdgeState::Active)
    );
    std::fs::remove_file(host.root.join("etc/app/app.env")).expect("remove env file");
    let scan = host.scan();
    assert_eq!(
        host.edge_state("app.service", EdgeKind::DependsOn, "postgresql.service"),
        Some(EdgeState::Stale),
        "an unreadable declaration is unknown, not a removal"
    );
    assert!(scan
        .warning_details
        .iter()
        .any(|detail| detail.detail.contains("declared file is missing")));
}

fn write_compose_fixture(docker: &std::path::Path) {
    let containers = docker.join("containers");
    std::fs::create_dir_all(&containers).expect("containers");
    let labels = |service: &str| {
        serde_json::json!({
            "com.docker.compose.project": "shop",
            "com.docker.compose.service": service,
            "com.docker.compose.project.config_files": "/srv/shop/compose.yml",
        })
    };
    let list: Vec<_> = ["web", "db"]
        .iter()
        .map(|name| {
            serde_json::json!({
                "Id": format!("id{name}"), "Names": [format!("/shop-{name}-1")],
                "Image": "app:1", "Labels": labels(name), "Ports": [], "Mounts": [],
            })
        })
        .collect();
    std::fs::write(
        containers.join("list.json"),
        serde_json::to_string(&list).expect("json"),
    )
    .expect("list");
    for name in ["web", "db"] {
        std::fs::write(
            containers.join(format!("id{name}.json")),
            serde_json::json!({
                "Id": format!("id{name}"), "Name": format!("/shop-{name}-1"),
                "Config": {"Image": "app:1", "Labels": labels(name)},
                "State": {"Status": "running", "Pid": 0},
                "NetworkSettings": {"Ports": {}}, "Mounts": [],
            })
            .to_string(),
        )
        .expect("inspect");
    }
}

#[test]
fn compose_depends_on_and_service_urls_link_containers_without_storing_secrets() {
    let host = DeclaredHost::new();
    let docker = host.home.layout.data_dir.join("docker");
    write_compose_fixture(&docker);
    std::fs::create_dir_all(host.root.join("srv/shop")).expect("compose dir");
    std::fs::write(
        host.root.join("srv/shop/compose.yml"),
        format!(
            "services:\n  web:\n    depends_on: [db]\n    environment:\n      DATABASE_URL: postgres://u:{SENTINEL}@db:5432/shop\n      SECRET_KEY: {SENTINEL}\n  db:\n    environment:\n      POSTGRES_PASSWORD: {SENTINEL}\n"
        ),
    )
    .expect("compose");
    let scan = {
        let _env = support::lock_scan_env();
        support::set_systemd_unit_root(&host.units);
        support::set_host_root(&host.root);
        let result = scan_in(
            &host.home.layout,
            twin_app::ScanRequest {
                docker_fixture: Some(docker),
                ..twin_app::ScanRequest::default()
            },
            &host.proc,
        );
        support::clear_scan_env();
        result.expect("scan")
    };
    assert!(!serde_json::to_string(&scan)
        .expect("json")
        .contains(SENTINEL));
    let store = Store::open(&host.home.layout.db_file()).expect("store");
    let containers = store
        .list_nodes_by_kind_typed(twin_core::NodeKind::Container)
        .expect("containers");
    let id = |service: &str| {
        containers
            .iter()
            .find(|node| {
                node.metadata()
                    .as_str()
                    .contains(&format!("\"compose_service\":\"{service}\""))
            })
            .map(|node| node.id().clone())
            .expect("compose container")
    };
    let edge = store
        .get_edge_typed(&EdgeId::new(&id("web"), EdgeKind::DependsOn, &id("db")))
        .expect("read")
        .expect("web depends on db");
    assert_eq!(edge.state(), EdgeState::Active);
    assert!(
        edge.metadata().as_str().contains("compose_depends_on"),
        "{}",
        edge.metadata()
    );
    drop(store);
    for file in std::fs::read_dir(&host.home.layout.data_dir)
        .expect("dir")
        .flatten()
    {
        if file.path().to_string_lossy().contains("twin.db") {
            let bytes = std::fs::read(file.path()).expect("bytes");
            assert!(!bytes
                .windows(SENTINEL.len())
                .any(|w| w == SENTINEL.as_bytes()));
        }
    }
}

#[test]
fn a_declared_pool_listener_links_its_proxy_when_the_socket_is_not_observed() {
    let host = DeclaredHost::new();
    for unit in ["caddy.service", "php8.2-fpm.service"] {
        std::fs::write(host.units.join(unit), "[Service]\n").expect("unit");
    }
    for (directory, file, text) in [
        (
            "etc/caddy",
            "Caddyfile",
            "example.com {\n  php_fastcgi 127.0.0.1:9000\n}\n",
        ),
        (
            "etc/php/8.2/fpm/pool.d",
            "www.conf",
            "[www]\nlisten = 127.0.0.1:9000\nenv[DB_PASSWORD] = SENTINEL_DECLARED_SECRET\n",
        ),
    ] {
        std::fs::create_dir_all(host.root.join(directory)).expect("dir");
        std::fs::write(host.root.join(directory).join(file), text).expect("config");
    }
    host.scan();
    assert_eq!(
        host.edge_state("caddy.service", EdgeKind::ProxiesTo, "php8.2-fpm.service"),
        Some(EdgeState::Active)
    );
}
