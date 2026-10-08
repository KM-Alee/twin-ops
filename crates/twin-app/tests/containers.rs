mod support;

use std::path::Path;

use twin_app::{EmulateActionRequest, EmulateRequest, GraphRequest, InitRequest, ScanRequest};
use twin_core::{NodeId, RiskLevel};
use twin_store::Store;

use support::{
    clear_scan_env, lock_scan_env, write_proc_fixture_with_cgroup, write_socket_fd,
    write_tcp_table, IsolatedHome,
};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

struct ClearOnDrop;

impl Drop for ClearOnDrop {
    fn drop(&mut self) {
        clear_scan_env();
    }
}

fn set_docker_fixture(root: &Path) {
    // SAFETY: guarded by SCAN_ENV_LOCK in tests.
    unsafe {
        std::env::set_var("TWIN_DOCKER_FIXTURE", root);
    }
}

fn write_redis_fixture(root: &Path) {
    let containers = root.join("containers");
    let images = root.join("images");
    std::fs::create_dir_all(&containers).expect("containers");
    std::fs::create_dir_all(&images).expect("images");
    std::fs::write(
        containers.join("list.json"),
        r#"[{
            "Id": "abc123redis",
            "Names": ["/redis"],
            "Image": "redis:7",
            "Ports": [{"IP": "0.0.0.0", "PrivatePort": 6379, "PublicPort": 6379, "Type": "tcp"}],
            "Mounts": [{"Type": "volume", "Name": "redis-data", "Source": "/var/lib/docker/volumes/redis-data/_data", "Destination": "/data"}]
        }]"#,
    )
    .expect("list");
    std::fs::write(
        containers.join("abc123redis.json"),
        r#"{
            "Id": "abc123redis",
            "Name": "/redis",
            "Config": {"Image": "redis:7"},
            "State": {"Status": "running", "Pid": 91},
            "NetworkSettings": {"Ports": {"6379/tcp": [{"HostIp": "0.0.0.0", "HostPort": "6379"}]}},
            "Mounts": [{"Type": "volume", "Name": "redis-data", "Source": "/var/lib/docker/volumes/redis-data/_data", "Destination": "/data"}]
        }"#,
    )
    .expect("inspect");
    std::fs::write(
        images.join("redis:7.json"),
        r#"{"Id": "sha256:deadbeef", "RepoTags": ["redis:7"]}"#,
    )
    .expect("image");
}

fn write_api_proc(proc: &Path) {
    std::fs::create_dir_all(proc).expect("proc");
    write_proc_fixture_with_cgroup(
        proc,
        90,
        "90 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/api\0",
        None,
        Some("0::/system.slice/api.service\n"),
    );
    write_proc_fixture_with_cgroup(
        proc,
        91,
        "91 (redis) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/redis-server\0",
        None,
        None,
    );
    write_tcp_table(
        proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:C350 00000000:18EB 01 00000000:00000000 00:00000000 00000000     0        0 54321 1 0000000000000000 20 4 30 10 -1"
        ),
    );
    write_socket_fd(proc, 90, 3, 54321);
}

#[test]
fn scan_graphs_redis_and_restart_emulation_is_hypothetical() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-redis");
    write_api_proc(&proc);
    let docker = home.layout.data_dir.join("fixture-docker-redis");
    write_redis_fixture(&docker);
    let list = docker.join("containers/list.json");
    let inspect = docker.join("containers/abc123redis.json");
    let image = docker.join("images/redis:7.json");
    let list_before = std::fs::read(&list).expect("list bytes");
    let inspect_before = std::fs::read(&inspect).expect("inspect bytes");
    let image_before = std::fs::read(&image).expect("image bytes");
    set_docker_fixture(&docker);

    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    assert!(
        scan.warnings
            .iter()
            .all(|warning| warning.kind != "docker_unavailable"),
        "fixture should satisfy docker: {:?}",
        scan.warning_details
    );

    let store = Store::open(&home.layout.db_file()).expect("open");
    for id in [
        "container:redis",
        "image:redis:7",
        "port:tcp:0.0.0.0:6379",
        "directory:/data",
        "service:api.service",
        "process:pid:91",
    ] {
        assert!(store.get_node(id).expect("node").is_some(), "missing {id}");
    }
    for id in [
        "container:redis|runs_image|image:redis:7",
        "container:redis|maps_port|port:tcp:0.0.0.0:6379",
        "container:redis|mounts_volume|directory:/data",
        "container:redis|owns|process:pid:91",
        "service:api.service|connects_to|port:tcp:0.0.0.0:6379",
    ] {
        assert!(store.get_edge(id).expect("edge").is_some(), "missing {id}");
    }

    let graph = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::container("redis")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Container(view) = graph else {
        panic!("expected container graph");
    };
    assert_eq!(view.container.id, "container:redis");
    assert_eq!(
        view.image.as_ref().map(|image| image.id.as_str()),
        Some("image:redis:7")
    );
    assert!(view.ports.iter().any(|port| {
        port.container_port == 6379 && port.host_ip == "0.0.0.0" && port.host_port == 6379
    }));
    assert!(view
        .mounts
        .iter()
        .any(|mount| mount.id == "directory:/data"));
    assert!(view
        .processes
        .iter()
        .any(|process| process.id == "process:pid:91"));

    let edges_before = store.count_edges().expect("edges");
    let restarted = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::container("redis")),
                target_query: None,
                show_paths: false,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert_eq!(restarted.action, "restart");
    assert_eq!(restarted.target, "container:redis");
    assert!(!restarted.action_performed);
    assert_eq!(restarted.risk.level, RiskLevel::High);
    assert!(restarted
        .safety_statement
        .contains("No container was restarted."));
    assert!(restarted
        .transient_impacts
        .iter()
        .any(|impact| { impact.statement == "service:api.service connects to redis port" }));
    assert_eq!(std::fs::read(&list).expect("list after"), list_before);
    assert_eq!(
        std::fs::read(&inspect).expect("inspect after"),
        inspect_before
    );
    assert_eq!(std::fs::read(&image).expect("image after"), image_before);
    let store = Store::open(&home.layout.db_file()).expect("reopen");
    assert_eq!(store.count_edges().expect("edges after"), edges_before);
}

#[test]
fn missing_fixture_and_missing_socket_do_not_panic() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("empty-proc-docker");
    std::fs::create_dir_all(&proc).expect("proc");
    set_docker_fixture(Path::new("/no/such/twin-docker-fixture"));
    let missing_fixture =
        twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("missing fixture");
    assert!(
        missing_fixture
            .warnings
            .iter()
            .any(|warning| warning.kind == "docker_unavailable")
            || missing_fixture
                .coverage
                .unavailable_collectors
                .iter()
                .any(|name| name == "docker"),
        "warnings {:?}",
        missing_fixture.warnings
    );

    clear_scan_env();
    let missing_socket =
        twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("missing socket");
    if !Path::new("/var/run/docker.sock").exists() {
        assert!(
            missing_socket
                .warnings
                .iter()
                .any(|warning| warning.kind == "docker_unavailable")
                || missing_socket
                    .coverage
                    .unavailable_collectors
                    .iter()
                    .any(|name| name == "docker")
        );
    }
}

#[test]
fn doctor_containers_does_not_crash_without_docker() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let result = twin_app::doctor_flags_in(&home.layout, None, false, true, false).expect("doctor");
    let containers = result.containers.expect("containers section");
    if !Path::new("/var/run/docker.sock").exists() {
        assert!(!containers.socket_present);
        assert!(containers.detail.contains("socket not found"));
    }
}
