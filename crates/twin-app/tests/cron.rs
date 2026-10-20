mod support;

use twin_app::{impact_in, scan_in, ImpactRequest, InitRequest, ScanRequest};
use twin_core::NodeId;
use twin_store::Store;

const SENTINEL: &str = "SENTINEL_CRON_PASSWORD";
const REDIS_CHECKED: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:18EB 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 6000 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:D431 0100007F:18EB 01 00000000:00000000 00000000:00000000  00000000       0        0 6001 1 0000000000000000 100 0 0 10 0";
const REDIS_IDLE: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:18EB 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 6000 1 0000000000000000 100 0 0 10 0";

struct CronHost {
    home: support::IsolatedHome,
    proc: std::path::PathBuf,
    units: std::path::PathBuf,
    cron: std::path::PathBuf,
}

impl CronHost {
    fn new() -> Self {
        let home = support::IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
        let proc = home.layout.data_dir.join("proc");
        let units = home.layout.data_dir.join("units");
        let cron = home.layout.data_dir.join("cron-root");
        for directory in [&proc, &units, &cron.join("etc/cron.d")] {
            std::fs::create_dir_all(directory).expect("directory");
        }
        for unit in ["redis.service", "cronie.service", "nginx.service"] {
            std::fs::write(units.join(unit), "[Service]\n").expect("unit");
        }
        std::fs::write(
            cron.join("etc/cron.d/redis-check"),
            format!(
                "MAILTO=ops\nREDISCLI_AUTH={SENTINEL}\n*/5 * * * * root /usr/bin/redis-cli -a {SENTINEL} ping\n"
            ),
        )
        .expect("cron.d");
        std::fs::write(
            cron.join("etc/cron.d/nginx-rotate"),
            "0 4 * * * root systemctl restart nginx\n",
        )
        .expect("cron.d");
        for (pid, comm, unit) in [
            (700, "redis-server", "redis.service"),
            (800, "crond", "cronie.service"),
            (801, "redis-cli", "cronie.service"),
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
        support::write_tcp_table(&proc, "tcp", REDIS_CHECKED);
        support::write_socket_fd(&proc, 700, 6, 6000);
        support::write_socket_fd(&proc, 801, 3, 6001);
        Self {
            home,
            proc,
            units,
            cron,
        }
    }

    fn scan(&self) -> twin_app::ScanResult {
        let _env = support::lock_scan_env();
        support::set_systemd_unit_root(&self.units);
        let result = scan_in(
            &self.home.layout,
            ScanRequest {
                cron_root: Some(self.cron.clone()),
                ..support::host_isolated_scan(&self.home)
            },
            &self.proc,
        );
        support::clear_scan_env();
        result.expect("scan")
    }

    fn job_finishes(&self) {
        std::fs::remove_dir_all(self.proc.join("801")).expect("job exits");
        support::write_tcp_table(&self.proc, "tcp", REDIS_IDLE);
    }

    fn impact(&self, service: &str) -> twin_app::ImpactResult {
        impact_in(
            &self.home.layout,
            ImpactRequest {
                target: Some(NodeId::service(service)),
                ..ImpactRequest::default()
            },
        )
        .expect("impact")
    }
}

#[test]
fn a_cron_only_client_stays_a_dependent_and_names_its_jobs() {
    let host = CronHost::new();
    let first = host.scan();
    assert_eq!(first.cron_job_count, 2);
    host.job_finishes();
    host.scan();
    let impact = host.impact("redis.service");
    let cron = impact
        .direct_dependents
        .iter()
        .find(|dependent| dependent.id == "service:cronie.service")
        .expect("cron daemon is a dependent of redis");
    assert!(
        cron.reason.contains("last seen ")
            && cron
                .reason
                .contains("runs cron jobs: nginx-rotate:1 (0 4 * * *) runs systemctl, redis-check:3 (*/5 * * * *) runs redis-cli"),
        "{}",
        cron.reason
    );
}

#[test]
fn cron_jobs_keep_typed_facts_and_never_the_command_or_environment() {
    let host = CronHost::new();
    host.scan();
    host.job_finishes();
    host.scan();
    let store = Store::open(&host.home.layout.db_file()).expect("store");
    let job = store
        .get_node_typed(&NodeId::cron_job("/etc/cron.d/nginx-rotate", 1))
        .expect("read")
        .expect("job node");
    let metadata: serde_json::Value =
        serde_json::from_str(job.metadata().as_str()).expect("metadata");
    assert_eq!(
        metadata["operations"],
        serde_json::json!(["restart nginx.service"])
    );
    assert_eq!(metadata["user"], "root");
    assert_eq!(metadata["schedule"], "0 4 * * *");
    drop(store);

    let impact = serde_json::to_string(&host.impact("redis.service")).expect("json");
    assert!(!impact.contains(SENTINEL));
    let why = twin_app::why_in(
        &host.home.layout,
        twin_app::why_request("service:cronie.service", None).expect("request"),
    )
    .expect("why");
    assert!(!serde_json::to_string(&why)
        .expect("json")
        .contains(SENTINEL));
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
fn a_removed_job_is_gone_but_an_unreadable_source_keeps_it_as_stale() {
    let host = CronHost::new();
    host.scan();
    std::fs::remove_file(host.cron.join("etc/cron.d/nginx-rotate")).expect("remove job");
    host.scan();
    let store = Store::open(&host.home.layout.db_file()).expect("store");
    let state = |line: &str, number| {
        store
            .get_node_typed(&NodeId::cron_job(line, number))
            .expect("read")
            .expect("node")
            .state()
    };
    assert_eq!(
        state("/etc/cron.d/nginx-rotate", 1),
        twin_core::NodeState::Gone
    );
    assert_eq!(
        state("/etc/cron.d/redis-check", 3),
        twin_core::NodeState::Active
    );
    drop(store);

    use std::os::unix::fs::PermissionsExt;
    let cron_d = host.cron.join("etc/cron.d");
    std::fs::set_permissions(&cron_d, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    let hidden = std::fs::read_dir(&cron_d).is_err();
    host.scan();
    std::fs::set_permissions(&cron_d, std::fs::Permissions::from_mode(0o755)).expect("restore");
    if hidden {
        let store = Store::open(&host.home.layout.db_file()).expect("store");
        let node = store
            .get_node_typed(&NodeId::cron_job("/etc/cron.d/redis-check", 3))
            .expect("read")
            .expect("node");
        assert_eq!(
            node.state(),
            twin_core::NodeState::Stale,
            "an unreadable source is unknown, not a removal"
        );
    }
}
