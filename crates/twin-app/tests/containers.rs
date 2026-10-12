mod support;

use std::path::Path;

use twin_app::{EmulateActionRequest, EmulateRequest, GraphRequest, InitRequest, ScanRequest};
use twin_core::{NodeId, NodeState, RiskLevel};
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
fn docker_outage_keeps_container_rows() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-redis");
    write_api_proc(&proc);
    let docker = home.layout.data_dir.join("fixture-docker-redis");
    write_redis_fixture(&docker);
    set_docker_fixture(&docker);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan with docker");

    let store = Store::open(&home.layout.db_file()).expect("open");
    let container = store
        .get_node_typed(&NodeId::container("redis"))
        .expect("node")
        .expect("container");
    assert_eq!(container.state(), NodeState::Active);

    set_docker_fixture(Path::new("/no/such/twin-docker-fixture"));
    let scan = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan outage");
    assert!(
        scan.warnings
            .iter()
            .any(|warning| warning.kind == "docker_unavailable"),
        "warnings {:?}",
        scan.warnings
    );

    let store = Store::open(&home.layout.db_file()).expect("reopen");
    let container = store
        .get_node_typed(&NodeId::container("redis"))
        .expect("node")
        .expect("container");
    assert_eq!(
        container.state(),
        NodeState::Active,
        "a docker outage must not reap container rows"
    );
    let edge = store
        .get_edge("container:redis|runs_image|image:redis:7")
        .expect("edge")
        .expect("runs image edge");
    assert_eq!(edge.state, "active");
}

#[test]
fn doctor_containers_does_not_crash_without_docker() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let result = twin_app::doctor_flags_in(&home.layout, None, false, true, false).expect("doctor");
    let containers = result.containers.expect("containers section");
    assert!(containers.containerd.is_some());
    if !Path::new("/var/run/docker.sock").exists() {
        assert!(!containers.socket_present);
        assert!(containers.detail.contains("socket not found"));
    }
}

fn seed_node_row(store: &mut Store, id: &str, kind: &str, label: &str) {
    store
        .upsert_node(&twin_store::NodeRow {
            id: id.to_string(),
            kind: kind.to_string(),
            label: label.to_string(),
            state: "active".to_string(),
            first_seen_ns: 1,
            last_seen_ns: 1,
            valid_from_ns: 1,
            valid_to_ns: None,
            metadata_json: "{}".to_string(),
        })
        .expect("node");
}

fn seed_edge_row(store: &mut Store, from: &str, kind: &str, to: &str, class: &str) {
    store
        .upsert_edge(&twin_store::EdgeRow {
            id: format!("{from}|{kind}|{to}"),
            from_node_id: from.to_string(),
            to_node_id: to.to_string(),
            kind: kind.to_string(),
            class: class.to_string(),
            state: "active".to_string(),
            evidence_score: 0,
            evidence_label: "weak".to_string(),
            evidence_count: 0,
            first_seen_ns: 1,
            last_seen_ns: 1,
            metadata_json: "{}".to_string(),
        })
        .expect("edge");
}

fn container_restart_request(home: &IsolatedHome, show_paths: bool) -> twin_app::EmulationResult {
    twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::container("redis")),
                target_query: None,
                show_paths,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate")
}

#[test]
fn container_restart_paths_list_transitive_dependents_of_affected_services() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    seed_node_row(&mut store, "container:redis", "container", "redis");
    seed_node_row(
        &mut store,
        "port:tcp:0.0.0.0:6379",
        "port",
        "tcp:0.0.0.0:6379",
    );
    seed_node_row(&mut store, "service:api.service", "service", "api.service");
    seed_node_row(
        &mut store,
        "service:worker.service",
        "service",
        "worker.service",
    );
    seed_edge_row(
        &mut store,
        "container:redis",
        "maps_port",
        "port:tcp:0.0.0.0:6379",
        "observed",
    );
    seed_edge_row(
        &mut store,
        "service:api.service",
        "connects_to",
        "port:tcp:0.0.0.0:6379",
        "inferred",
    );
    seed_edge_row(
        &mut store,
        "service:worker.service",
        "depends_on",
        "service:api.service",
        "inferred",
    );

    let direct = container_restart_request(&home, false);
    assert!(!direct.paths_requested);
    assert_eq!(direct.impact_paths.len(), 1);
    assert!(direct
        .overlay
        .unavailable_nodes
        .iter()
        .any(|node| node.id == "service:worker.service"));
    assert!(direct
        .overlay
        .unavailable_nodes
        .iter()
        .any(|node| node.id == "service:api.service"));
    assert!(store
        .get_node("service:worker.service")
        .expect("node")
        .is_some_and(|node| node.state == "active"));

    let with_paths = container_restart_request(&home, true);
    assert!(with_paths.paths_requested);
    assert_eq!(with_paths.max_depth, 4);
    assert_eq!(with_paths.impact_paths.len(), 1);
    let path = &with_paths.impact_paths[0];
    assert_eq!(path.terminal_id, "service:worker.service");
    assert_eq!(path.terminal_label, "worker.service");
    assert_eq!(path.depth, 1);
    assert_eq!(
        path.path_chain,
        "service:worker.service -> service:api.service"
    );
    assert!(with_paths
        .unknowns
        .iter()
        .any(|unknown| unknown.kind == "missing_evidence"));
}

#[test]
fn missing_container_inspection_is_degraded_and_preserves_ownership_as_stale() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let docker = home.layout.data_dir.join("docker");
    write_redis_fixture(&docker);
    set_docker_fixture(&docker);
    let proc = home.layout.data_dir.join("proc");
    write_api_proc(&proc);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("first");
    std::fs::remove_file(docker.join("containers/abc123redis.json")).expect("inspect missing");
    let result = twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("partial");
    assert!(result
        .warnings
        .iter()
        .any(|warning| warning.kind == "docker_inspect_incomplete"));
    let store = Store::open(&home.layout.db_file()).expect("store");
    let owns = store.list_edges_from("container:redis").expect("edges");
    assert!(owns
        .iter()
        .any(|edge| edge.to_node_id == "process:pid:91" && edge.state == "stale"));
    let runs = store.list_collector_runs(Some("docker")).expect("runs");
    assert_eq!(runs.last().expect("run").status, "degraded");
}

#[test]
fn replacement_container_preserves_alias_but_starts_a_new_lifetime() {
    let _env = lock_scan_env();
    let _clear = ClearOnDrop;
    clear_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let docker = home.layout.data_dir.join("docker");
    write_redis_fixture(&docker);
    set_docker_fixture(&docker);
    let proc = home.layout.data_dir.join("proc");
    write_api_proc(&proc);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("first");
    let store = Store::open(&home.layout.db_file()).expect("store");
    let first = store
        .get_node("container:redis")
        .expect("query")
        .expect("container");
    let list = std::fs::read_to_string(docker.join("containers/list.json"))
        .expect("list")
        .replace("abc123redis", "replacement");
    std::fs::write(docker.join("containers/list.json"), list).expect("new list");
    let inspect = std::fs::read_to_string(docker.join("containers/abc123redis.json"))
        .expect("inspect")
        .replace("abc123redis", "replacement")
        .replace(
            "\"Config\":",
            "\"Image\":\"sha256:actual-running-artifact\",\"Config\":",
        );
    std::fs::write(docker.join("containers/replacement.json"), inspect).expect("replacement");
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("replacement scan");
    let replacement = store
        .get_node("container:redis")
        .expect("query")
        .expect("container");
    assert!(replacement.first_seen_ns > first.first_seen_ns);
    assert!(replacement.metadata_json.contains("replacement"));
    assert!(replacement
        .metadata_json
        .contains("sha256:actual-running-artifact"));
}
