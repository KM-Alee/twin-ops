mod support;

use std::fmt::Write as _;

use support::{
    stderr_utf8, write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, TwinHome,
};

const SERVICES: u32 = 40;
const PROCESSES_PER_SERVICE: u32 = 25;
const CONNECTIONS_PER_PROCESS: u32 = 4;
const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

/// 1 000 processes in 40 services, each listening on its own port and holding four
/// connections to the next service: a busy host for output-size regressions.
fn busy_host() -> TwinHome {
    let home = TwinHome::new();
    let mut tcp = String::from(TCP_HEADER);
    let mut line = 0u32;
    let mut inode = 100_000u64;
    for service in 0..SERVICES {
        let port = 10_000 + service;
        let leader = 1_000 + service * PROCESSES_PER_SERVICE;
        inode += 1;
        let _ = write!(
            tcp,
            "\n {line:>4}: 00000000:{port:04X} 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 {inode} 1 0000000000000000 100 0 0 10 0"
        );
        line += 1;
        write_socket_fd(&home.proc_root, leader, 3, inode);
        for offset in 0..PROCESSES_PER_SERVICE {
            let pid = leader + offset;
            write_proc_fixture_with_cgroup(
                &home.proc_root,
                pid,
                &format!("{pid} (svc{service}) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
                "Uid:\t0\t0\t0\t0\n",
                format!("/usr/bin/svc{service}\0").as_bytes(),
                None,
                Some(&format!("0::/system.slice/svc{service}.service\n")),
            );
            let target = 10_000 + (service + 1) % SERVICES;
            for connection in 0..CONNECTIONS_PER_PROCESS {
                inode += 1;
                let local_port = 30_000 + offset * CONNECTIONS_PER_PROCESS + connection;
                let _ = write!(
                    tcp,
                    "\n {line:>4}: 0100007F:{local_port:04X} 0100007F:{target:04X} 01 00000000:00000000 00000000:00000000  00000000       0        0 {inode} 1 0000000000000000 100 0 0 10 0"
                );
                line += 1;
                write_socket_fd(&home.proc_root, pid, 10 + connection, inode);
            }
        }
    }
    write_tcp_table(&home.proc_root, "tcp", &tcp);
    for args in [&["init"][..], &["scan"], &["scan"]] {
        let output = home.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            stderr_utf8(&output)
        );
    }
    home
}

#[test]
fn json_output_stays_within_ceilings_and_reports_omissions_on_a_busy_host() {
    let home = busy_host();
    let ceilings: [(&[&str], usize); 10] = [
        (&["--json", "scan"], 64 * 1024),
        (&["--json", "graph", "--kind", "process"], 64 * 1024),
        (&["--json", "graph", "--kind", "port"], 64 * 1024),
        (&["--json", "graph", "svc7"], 96 * 1024),
        (&["--json", "what-changed", "--since", "1h"], 64 * 1024),
        (&["--json", "impact", "svc7"], 64 * 1024),
        (&["--json", "emulate", "restart", "svc7"], 64 * 1024),
        (&["--json", "why", "svc7"], 96 * 1024),
        (&["--json", "brief", "--max-age", "3600"], 16 * 1024),
        (&["--json", "brief", "svc7", "--max-age", "3600"], 16 * 1024),
    ];
    for (args, ceiling) in ceilings {
        let output = home.run(args);
        assert!(
            output.status.success(),
            "{args:?}: {}",
            stderr_utf8(&output)
        );
        let size = output.stdout.len();
        assert!(size <= ceiling, "{args:?}: {size} bytes exceeds {ceiling}");
        serde_json::from_slice::<serde_json::Value>(&output.stdout)
            .unwrap_or_else(|error| panic!("{args:?}: invalid json: {error}"));
    }

    let bounded = home.run(&["--json", "graph", "--kind", "process"]);
    let value: serde_json::Value = serde_json::from_slice(&bounded.stdout).expect("json");
    assert_eq!(value["nodes"].as_array().expect("nodes").len(), 100);
    let omitted = value["nodes_omitted"].as_u64().expect("omitted count");
    let full = home.run(&["--json", "--full", "graph", "--kind", "process"]);
    let value: serde_json::Value = serde_json::from_slice(&full.stdout).expect("json");
    let total = value["nodes"].as_array().expect("nodes").len() as u64;
    assert_eq!(total, 100 + omitted);
    assert!(value.get("nodes_omitted").is_none());

    let changes = home.run(&["--json", "what-changed", "--since", "1h"]);
    let value: serde_json::Value = serde_json::from_slice(&changes.stdout).expect("json");
    assert!(value.get("new_nodes").is_none());
    assert!(value["raw_change_counts"]["new_nodes"].as_u64().is_some());
}
