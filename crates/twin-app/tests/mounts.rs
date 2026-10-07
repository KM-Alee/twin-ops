mod support;

use twin_app::{EmulateActionRequest, EmulateRequest, GraphRequest, InitRequest, ScanRequest};
use twin_core::NodeId;
use twin_store::Store;

use support::{
    clear_scan_env, lock_scan_env, set_host_root, write_proc_fixture_with_cgroup, IsolatedHome,
};

fn write_service(proc: &std::path::Path, pid: u32, name: &str, unit: &str) {
    write_proc_fixture_with_cgroup(
        proc,
        pid,
        &format!("{pid} ({name}) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
        "Uid:\t0\t0\t0\t0\n",
        format!("/usr/bin/{name}\0").as_bytes(),
        None,
        Some(&format!("0::/system.slice/{unit}\n")),
    );
}

#[test]
fn scan_records_mount_usage_and_service_paths() {
    let _env = lock_scan_env();
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-mounts");
    std::fs::create_dir_all(&proc).expect("proc");
    write_service(&proc, 80, "postgres", "postgresql.service");
    write_service(&proc, 81, "dockerd", "docker.service");
    write_service(&proc, 82, "journald", "systemd-journald.service");
    std::fs::write(
        proc.join("mounts"),
        "/dev/sda1 /var ext4 rw,relatime 0 0\nproc /proc proc rw 0 0\nbogus line\n",
    )
    .expect("mounts");
    let host = home.layout.data_dir.join("fixture-host-mounts");
    for path in [
        "var/lib/postgresql",
        "var/lib/docker",
        "var/log/journal",
        "var/log",
    ] {
        std::fs::create_dir_all(host.join(path)).expect("dir");
    }
    let marker = host.join("var/lib/postgresql/PG_VERSION");
    std::fs::write(&marker, "14\n").expect("marker");
    set_host_root(&host);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
    clear_scan_env();

    let store = Store::open(&home.layout.db_file()).expect("open");
    let mount = store
        .get_node("mount:/var")
        .expect("node")
        .expect("mount node");
    assert!(
        mount.metadata_json.contains("used_percent"),
        "{}",
        mount.metadata_json
    );
    assert!(store
        .get_node("directory:/var/lib/postgresql")
        .expect("node")
        .is_some());
    assert!(store
        .get_node("directory:/var/log")
        .expect("node")
        .is_some());
    assert!(store
        .get_edge("directory:/var/lib/postgresql|mounted_on|mount:/var")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:postgresql.service|uses|directory:/var/lib/postgresql")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:docker.service|uses|directory:/var/lib/docker")
        .expect("edge")
        .is_some());
    assert!(store
        .get_edge("service:systemd-journald.service|logs_to|directory:/var/log/journal")
        .expect("edge")
        .is_some());

    let graph = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::mount("/var")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let twin_app::GraphResult::Mount(view) = graph else {
        panic!("expected mount graph");
    };
    assert!(view.used_percent.is_some());
    assert!(view
        .affected
        .iter()
        .any(|node| node.tag.as_deref() == Some("uses /var/lib/postgresql")));

    let edges_before = store.count_edges().expect("edges");
    let before = std::fs::read(&marker).expect("read marker");
    let filled = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::FillDisk {
                mount: "/var".to_string(),
                to_percent: 95,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("fill");
    assert_eq!(filled.action, "fill-disk");
    assert!(!filled.action_performed);
    assert_eq!(filled.risk.level, twin_core::RiskLevel::Critical);
    assert!(filled.persistent_impacts.iter().any(|impact| impact
        .statement
        .contains("postgresql.service uses /var/lib/postgresql")));
    assert!(filled
        .persistent_impacts
        .iter()
        .any(|impact| impact.statement.contains("writes to /var/log/journal")));
    assert!(filled.persistent_impacts.iter().any(|impact| impact
        .statement
        .contains("docker.service uses /var/lib/docker")));
    assert_eq!(std::fs::read(&marker).expect("marker after"), before);
    let store = Store::open(&home.layout.db_file()).expect("reopen");
    assert_eq!(store.count_edges().expect("edges after"), edges_before);
}

#[test]
fn relative_fill_path_is_rejected() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let err = twin_app::emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::FillDisk {
                mount: "var".to_string(),
                to_percent: 95,
            },
            ..EmulateRequest::default()
        },
    )
    .expect_err("relative");
    let message = err.to_string();
    assert!(message.contains("relative"), "{message}");
}
