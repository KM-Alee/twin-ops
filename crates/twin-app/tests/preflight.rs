mod support;

use std::path::{Path, PathBuf};

use twin_app::{PreflightRequest, PreflightResult, PreflightVerdict};
use twin_core::{GraphMetadata, GraphNode, GraphNodeParts, NodeId, TimestampNs};

use support::IsolatedHome;

fn request(command: &str) -> PreflightRequest {
    PreflightRequest {
        command: command.to_string(),
        file_changes: Vec::new(),
        cwd: Some(PathBuf::from("/home/dev/project")),
        config_override: None,
        max_graph_age_secs: u64::MAX,
        should_read_scripts: true,
    }
}

fn run(home: &IsolatedHome, proc: &Path, command: &str) -> PreflightResult {
    twin_app::preflight_in(&home.layout, &request(command), proc).expect("preflight")
}

fn scanned_home() -> (IsolatedHome, PathBuf) {
    let home = IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    (home, proc)
}

#[test]
fn non_operational_commands_are_go_without_touching_the_store() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("unused-proc");
    let result = run(
        &home,
        &proc,
        "ls -la && cargo build 2>/dev/null | tee build.log",
    );
    assert_eq!(result.verdict, PreflightVerdict::Go);
    assert!(!home.layout.db_file().exists());
    assert!(!result.action_performed);
    assert!(result.graph_age_secs.is_none());
}

#[test]
fn host_power_and_root_deletion_stop_without_a_graph() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("unused-proc");
    for command in ["sudo reboot", "systemctl poweroff", "shutdown -h now"] {
        assert_eq!(
            run(&home, &proc, command).verdict,
            PreflightVerdict::Stop,
            "{command}"
        );
    }
}

#[test]
fn operational_command_without_initialized_store_is_an_error() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("unused-proc");
    assert!(
        twin_app::preflight_in(&home.layout, &request("systemctl restart nginx"), &proc).is_err()
    );
}

#[test]
fn stopping_a_service_with_runtime_dependents_is_a_stop() {
    let (home, proc) = scanned_home();
    let result = run(&home, &proc, "sudo systemctl stop postgresql");
    assert_eq!(result.verdict, PreflightVerdict::Stop);
    let step = &result.steps[0];
    assert!(step.is_assessed);
    assert_eq!(step.target.as_deref(), Some("service:postgresql.service"));
    assert!(step
        .affected
        .iter()
        .any(|affected| affected.id == "service:django.service"));
    assert!(step
        .reasons
        .iter()
        .any(|reason| reason.contains("0.0.0.0:5432")));
}

#[test]
fn restart_reports_interrupted_dependents_and_reload_is_go() {
    let (home, proc) = scanned_home();
    let restart = run(&home, &proc, "systemctl restart postgresql.service");
    assert!(restart.verdict >= PreflightVerdict::Caution);
    assert!(restart.steps[0]
        .affected
        .iter()
        .any(|affected| affected.label == "django.service"));
    let reload = run(&home, &proc, "systemctl reload postgresql");
    assert_eq!(reload.verdict, PreflightVerdict::Go);
}

#[test]
fn wrappers_nested_shells_and_compound_lines_are_all_evaluated() {
    let (home, proc) = scanned_home();
    let result = run(
        &home,
        &proc,
        "cd /srv && PGHOST=x sudo -u root bash -c 'echo ok; systemctl stop postgresql'",
    );
    assert_eq!(result.verdict, PreflightVerdict::Stop);
    assert!(result
        .steps
        .iter()
        .any(|step| step.target.as_deref() == Some("service:postgresql.service")));
}

#[test]
fn killing_a_pid_is_attributed_to_its_owning_service() {
    let (home, proc) = scanned_home();
    let result = run(&home, &proc, "kill -9 721");
    assert_eq!(result.verdict, PreflightVerdict::Stop);
    assert!(result.steps[0].operation.contains("postgresql.service"));
    let unknown = run(&home, &proc, "kill 99999");
    assert_eq!(unknown.verdict, PreflightVerdict::Go);
}

#[test]
fn deleting_untracked_paths_depends_on_where_they_live() {
    let (home, proc) = scanned_home();
    assert_eq!(
        run(&home, &proc, "rm -rf ./target node_modules").verdict,
        PreflightVerdict::Go
    );
    assert_eq!(
        run(&home, &proc, "sudo rm /etc/untracked.conf").verdict,
        PreflightVerdict::Caution
    );
    assert_eq!(
        run(&home, &proc, "rm -rf /").verdict,
        PreflightVerdict::Stop
    );
    assert_eq!(
        run(&home, &proc, "rm -rf /usr/*").verdict,
        PreflightVerdict::Stop
    );
}

#[test]
fn redirection_into_a_system_file_is_an_edit() {
    let (home, proc) = scanned_home();
    let result = run(&home, &proc, "echo 'nameserver 1.1.1.1' > /etc/resolv.conf");
    assert_eq!(result.verdict, PreflightVerdict::Caution);
    assert!(result
        .steps
        .iter()
        .any(|step| step.operation == "modify /etc/resolv.conf"));
}

#[test]
fn unknown_service_cannot_be_assessed_and_is_caution() {
    let (home, proc) = scanned_home();
    let result = run(&home, &proc, "systemctl restart does-not-exist");
    assert_eq!(result.verdict, PreflightVerdict::Caution);
    assert!(!result.steps[0].is_assessed);
}

#[test]
fn restarting_docker_stops_running_containers_unless_live_restore_is_enabled() {
    let (home, proc) = scanned_home();
    let now = TimestampNs::now();
    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    store
        .upsert_node_typed(&GraphNode::service("docker.service", now, None))
        .expect("docker service");
    let container = |name: &str, live_restore: bool| {
        let node = GraphNode::container(name, now, None);
        GraphNode::from_parts(GraphNodeParts {
            id: node.id().clone(),
            kind: node.kind(),
            label: node.label().into(),
            state: node.state(),
            first_seen: node.first_seen(),
            last_seen: node.last_seen(),
            valid_from: node.valid_from(),
            valid_to: node.valid_to(),
            metadata: GraphMetadata::from_json(
                serde_json::json!({
                    "source": "docker",
                    "is_running": true,
                    "daemon_live_restore": live_restore,
                })
                .to_string(),
            ),
        })
    };
    store
        .upsert_node_typed(&container("redis", false))
        .expect("redis");
    drop(store);
    let result = run(&home, &proc, "systemctl restart docker");
    assert_eq!(result.verdict, PreflightVerdict::Stop);
    assert!(result.steps[0]
        .affected
        .iter()
        .any(|affected| affected.id == NodeId::container("redis").to_string()));

    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    store
        .upsert_node_typed(&container("redis", true))
        .expect("redis live restore");
    drop(store);
    let survived = run(&home, &proc, "systemctl restart docker");
    assert!(survived.steps[0].affected.is_empty());
    assert!(survived.steps[0]
        .reasons
        .iter()
        .any(|reason| reason.contains("live-restore")));
}

#[test]
fn stale_graph_is_rescanned_before_assessment() {
    let (home, proc) = scanned_home();
    let mut fresh = request("systemctl restart postgresql");
    fresh.max_graph_age_secs = 0;
    std::thread::sleep(std::time::Duration::from_millis(1100));
    let result = twin_app::preflight_in(&home.layout, &fresh, &proc).expect("preflight");
    assert_eq!(result.graph_age_secs, Some(0));
}

#[test]
fn direct_file_changes_are_assessed_like_shell_edits() {
    let (home, proc) = scanned_home();
    let change = |path: &str, operation| twin_app::FileChange {
        path: PathBuf::from(path),
        operation,
    };
    let mut request = request("");
    request.file_changes = vec![
        change("src/main.rs", twin_app::FileOperation::Edit),
        change("/etc/untracked.conf", twin_app::FileOperation::Edit),
    ];
    let result = twin_app::preflight_in(&home.layout, &request, &proc).expect("preflight");
    assert_eq!(result.verdict, PreflightVerdict::Caution);
    assert!(result.command.contains("edit /etc/untracked.conf"));
    assert!(result.steps.iter().any(|step| step.operation
        == "change /home/dev/project/src/main.rs"
        || step.operation == "modify /home/dev/project/src/main.rs"));

    request.file_changes = vec![change("src/main.rs", twin_app::FileOperation::Edit)];
    let project_only = twin_app::preflight_in(&home.layout, &request, &proc).expect("preflight");
    assert_eq!(project_only.verdict, PreflightVerdict::Go);
}

fn put_node(
    store: &mut twin_store::Store,
    id: &str,
    kind: &str,
    label: &str,
    metadata: serde_json::Value,
) {
    let now = TimestampNs::now().as_i64();
    store
        .upsert_node(&twin_store::NodeRow {
            id: id.into(),
            kind: kind.into(),
            label: label.into(),
            state: "active".into(),
            first_seen_ns: now,
            last_seen_ns: now,
            valid_from_ns: now,
            valid_to_ns: None,
            metadata_json: metadata.to_string(),
        })
        .expect("node");
}

fn put_edge(store: &mut twin_store::Store, from: &str, kind: &str, to: &str) {
    put_edge_with(store, from, kind, to, "inferred", serde_json::json!({}));
}

fn put_edge_with(
    store: &mut twin_store::Store,
    from: &str,
    kind: &str,
    to: &str,
    class: &str,
    metadata: serde_json::Value,
) {
    let now = TimestampNs::now().as_i64();
    store
        .upsert_edge(&twin_store::EdgeRow {
            id: format!("{from}|{kind}|{to}"),
            from_node_id: from.into(),
            to_node_id: to.into(),
            kind: kind.into(),
            class: class.into(),
            state: "active".into(),
            evidence_score: 0,
            evidence_label: "weak".into(),
            evidence_count: 0,
            first_seen_ns: now,
            last_seen_ns: now,
            metadata_json: metadata.to_string(),
        })
        .expect("edge");
}

/// postgresql.service uses /srv/pg, which lives on the /srv mount backed by /dev/sdb1.
fn mounted_data_home() -> (IsolatedHome, PathBuf) {
    let (home, proc) = scanned_home();
    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    put_node(
        &mut store,
        "mount:/srv",
        "mount",
        "/srv",
        serde_json::json!({"device":"/dev/sdb1","fstype":"ext4"}),
    );
    put_node(
        &mut store,
        "directory:/srv/pg",
        "directory",
        "/srv/pg",
        serde_json::json!({}),
    );
    put_edge(&mut store, "directory:/srv/pg", "mounted_on", "mount:/srv");
    put_edge(
        &mut store,
        "service:postgresql.service",
        "uses",
        "directory:/srv/pg",
    );
    drop(store);
    (home, proc)
}

#[test]
fn unmounting_or_wiping_storage_in_use_is_a_stop() {
    let (home, proc) = mounted_data_home();
    for command in [
        "umount /srv",
        "umount /dev/sdb1",
        "mkfs.xfs -f /dev/sdb1",
        "dd if=/dev/zero of=/dev/sdb",
    ] {
        let result = run(&home, &proc, command);
        assert_eq!(
            result.verdict,
            PreflightVerdict::Stop,
            "{command}: {result:#?}"
        );
        assert!(
            result.steps[0]
                .affected
                .iter()
                .any(|affected| affected.label == "postgresql.service"),
            "{command}"
        );
    }
    assert_eq!(
        run(&home, &proc, "umount /").verdict,
        PreflightVerdict::Stop
    );
    assert_eq!(
        run(&home, &proc, "umount /mnt/usb").verdict,
        PreflightVerdict::Caution
    );
    assert_eq!(
        run(&home, &proc, "wipefs -a /dev/sdz").verdict,
        PreflightVerdict::Caution
    );
}

#[test]
fn interface_down_reads_routes_and_bound_listeners() {
    let (home, proc) = scanned_home();
    let net = proc.join("net");
    std::fs::write(
        net.join("route"),
        "Iface\tDestination\tGateway \tFlags\tRefCnt\tUse\tMetric\tMask\t\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t0100000A\t0003\t0\t0\t0\t00000000\t0\t0\t0\n\
eth0\t0000000A\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n\
eth1\t0000A8C0\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n",
    )
    .expect("route");
    std::fs::write(
        net.join("fib_trie"),
        "Main:\n  +-- 0.0.0.0/0 3 0 4\n     |-- 10.0.0.5\n        /32 host LOCAL\n     |-- 192.168.0.9\n        /32 host LOCAL\n",
    )
    .expect("fib_trie");
    let gateway = run(&home, &proc, "ip link set eth0 down");
    assert_eq!(gateway.verdict, PreflightVerdict::Stop);
    assert!(gateway.headline.contains("default route"));
    let quiet = run(&home, &proc, "ifdown eth1");
    assert_eq!(quiet.verdict, PreflightVerdict::Caution);
    assert!(quiet.steps[0]
        .reasons
        .iter()
        .any(|r| r.contains("wildcard")));
    assert_eq!(
        run(&home, &proc, "ip link set lo down").verdict,
        PreflightVerdict::Stop
    );
    assert_eq!(
        run(&home, &proc, "ip link set wg9 down").verdict,
        PreflightVerdict::Caution
    );
}

#[test]
fn compose_commands_resolve_project_containers_from_labels() {
    let (home, proc) = scanned_home();
    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    for (name, service) in [("shop-web-1", "web"), ("shop-db-1", "db")] {
        put_node(
            &mut store,
            &format!("container:{name}"),
            "container",
            name,
            serde_json::json!({"source":"docker","is_running":true,"compose_project":"shop","compose_service":service}),
        );
    }
    drop(store);
    let mut down = request("docker compose down");
    down.cwd = Some(PathBuf::from("/srv/Shop"));
    let result = twin_app::preflight_in(&home.layout, &down, &proc).expect("preflight");
    assert!(result.verdict >= PreflightVerdict::Caution);
    assert_eq!(result.steps[0].affected_total, 2, "{result:#?}");
    assert!(result.headline.contains("project shop"));

    let one = run(&home, &proc, "docker compose -p shop restart web");
    assert_eq!(one.steps[0].affected.len(), 1);
    assert_eq!(one.steps[0].affected[0].label, "shop-web-1");

    let unknown = run(&home, &proc, "docker compose -p nothing down");
    assert_eq!(unknown.verdict, PreflightVerdict::Caution);
    assert!(!unknown.steps[0].is_assessed);
}

#[test]
fn scripts_and_make_recipes_are_evaluated_line_by_line_without_leaking_other_lines() {
    let (home, proc) = scanned_home();
    let project = home.layout.data_dir.join("project");
    std::fs::create_dir_all(&project).expect("project");
    std::fs::write(
        project.join("deploy.sh"),
        "#!/usr/bin/env bash\nset -e\n# systemctl stop postgresql (comment only)\ncurl -H 'Authorization: Bearer SCRIPT_SECRET' https://example.invalid\ncat <<EOF > notes.txt\nrm -rf /\nEOF\nsudo systemctl \\\n  stop postgresql\nsystemctl restart \"$SERVICE\"\n",
    )
    .expect("script");
    std::fs::write(
        project.join("Makefile"),
        "IMAGE = app\n\n.PHONY: release build\nrelease: build\n\t@systemctl restart postgresql\n\nbuild:\n\tcargo build --release\n\nclean:\n\trm -rf /var/lib/app\n",
    )
    .expect("makefile");
    let mut script = request("./deploy.sh --fast");
    script.cwd = Some(project.clone());
    let result = twin_app::preflight_in(&home.layout, &script, &proc).expect("preflight");
    assert_eq!(result.verdict, PreflightVerdict::Stop, "{result:#?}");
    let json = serde_json::to_string(&result).expect("json");
    assert!(
        !json.contains("SCRIPT_SECRET"),
        "script lines that are not operations leak"
    );
    assert!(
        !json.contains("rm -rf /"),
        "heredoc bodies are data, not commands"
    );
    assert!(result
        .steps
        .iter()
        .any(|step| step.segment.starts_with("deploy.sh:8:")));
    assert!(result.steps.iter().any(|step| step
        .reasons
        .iter()
        .any(|reason| reason.contains("shell variables"))));

    let mut make = request("make release");
    make.cwd = Some(project.clone());
    let release = twin_app::preflight_in(&home.layout, &make, &proc).expect("make");
    assert!(release.verdict >= PreflightVerdict::Caution);
    assert!(release
        .steps
        .iter()
        .any(|step| step.target.as_deref() == Some("service:postgresql.service")));
    assert!(!serde_json::to_string(&release)
        .expect("json")
        .contains("/var/lib/app"));

    let mut missing = request("./nope.sh");
    missing.cwd = Some(project);
    let unreadable = twin_app::preflight_in(&home.layout, &missing, &proc).expect("missing");
    assert_eq!(unreadable.verdict, PreflightVerdict::Caution);
}

#[test]
fn declared_requires_of_a_service_running_processes_is_runtime_without_dbus() {
    let (home, proc) = scanned_home();
    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    for (unit, pid) in [("orders.service", 9001), ("reports.service", 9002)] {
        put_node(
            &mut store,
            &format!("service:{unit}"),
            "service",
            unit,
            serde_json::json!({}),
        );
        put_node(
            &mut store,
            &format!("process:pid:{pid}"),
            "process",
            unit,
            serde_json::json!({}),
        );
        put_edge(
            &mut store,
            &format!("service:{unit}"),
            "owns",
            &format!("process:pid:{pid}"),
        );
    }
    put_node(
        &mut store,
        "service:ledger.service",
        "service",
        "ledger.service",
        serde_json::json!({}),
    );
    put_edge_with(
        &mut store,
        "service:orders.service",
        "depends_on",
        "service:ledger.service",
        "observed",
        serde_json::json!({"key": "Requires"}),
    );
    put_edge_with(
        &mut store,
        "service:reports.service",
        "depends_on",
        "service:ledger.service",
        "observed",
        serde_json::json!({"key": "Wants"}),
    );
    drop(store);
    let result = run(&home, &proc, "systemctl stop ledger");
    assert_eq!(result.verdict, PreflightVerdict::Stop, "{result:#?}");
    let affected = result.steps[0]
        .affected
        .iter()
        .map(|a| a.label.as_str())
        .collect::<Vec<_>>();
    assert!(affected.contains(&"orders.service"), "{affected:?}");
    assert!(
        !affected.contains(&"reports.service"),
        "Wants alone is not runtime: {affected:?}"
    );
}
