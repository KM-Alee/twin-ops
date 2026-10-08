use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorEbpf, DoctorEbpfCheck, DoctorPermissions, DoctorResult,
    PermissionMode, WatchExecEvent, WatchTick,
};
use twin_cli::output;

fn check(ok: bool, status: &str) -> DoctorEbpfCheck {
    DoctorEbpfCheck {
        ok,
        status: status.to_string(),
        detail: if ok {
            None
        } else {
            Some(format!("{status} detail"))
        },
    }
}

fn doctor_with(ebpf: DoctorEbpf) -> DoctorResult {
    DoctorResult {
        core: DoctorCore {
            cli_ok: true,
            config_found: true,
            config_path: Some(PathBuf::from("/tmp/config.toml")),
        },
        database: DoctorDatabase {
            initialized: true,
            schema_version: Some(4),
            db_path: Some(PathBuf::from("/tmp/twin.db")),
            wal_mode: Some(true),
        },
        permissions: DoctorPermissions {
            mode: PermissionMode::Unprivileged,
            proc_accessible: true,
            readable_process_count: Some(1),
            restricted_process_count: Some(0),
        },
        scan_quality: None,
        scan_quality_error: None,
        ebpf: Some(ebpf),
        containers: None,
        k8s: None,
    }
}

fn ready_ebpf() -> DoctorEbpf {
    DoctorEbpf {
        kernel: check(true, "supported"),
        btf: check(true, "available"),
        capabilities: check(true, "available"),
        exec_tracing: check(true, "available"),
        tcp_tracing: check(true, "available"),
    }
}

fn line<'a>(text: &'a str, label: &str) -> &'a str {
    text.lines()
        .find(|line| line.contains(label))
        .unwrap_or_else(|| panic!("missing {label} in {text}"))
}

#[test]
fn doctor_ebpf_text_and_json_include_four_checks() {
    let result = doctor_with(ready_ebpf());
    let text = output::doctor::render(&result);
    assert!(text.contains("eBPF"));
    assert!(line(&text, "kernel").contains("ok"));
    assert!(line(&text, "kernel").contains("supported"));
    assert!(line(&text, "BTF").contains("available"));
    assert!(line(&text, "capabilities").contains("available"));
    assert!(line(&text, "exec tracing").contains("available"));
    assert!(!line(&text, "kernel").contains("warn"));

    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    for key in ["kernel", "btf", "capabilities", "exec_tracing"] {
        assert!(value["ebpf"][key]["ok"].as_bool().expect(key));
        assert!(value["ebpf"][key]["status"].is_string());
    }
}

#[test]
fn each_failed_ebpf_check_renders_a_warn_tag() {
    let cases = [
        ("kernel", "unsupported", 0),
        ("BTF", "unavailable", 1),
        ("capabilities", "unavailable", 2),
        ("exec tracing", "unavailable", 3),
        ("tcp tracing", "unavailable", 4),
    ];
    for (label, status, index) in cases {
        let mut ebpf = ready_ebpf();
        let failed = check(false, status);
        match index {
            0 => ebpf.kernel = failed,
            1 => ebpf.btf = failed,
            2 => ebpf.capabilities = failed,
            3 => ebpf.exec_tracing = failed,
            _ => ebpf.tcp_tracing = failed,
        }
        let text = output::doctor::render(&doctor_with(ebpf));
        let row = line(&text, label);
        assert!(row.contains("warn"), "{label}: {row}");
        assert!(row.contains(status), "{label}: {row}");
    }
}

#[test]
fn watch_exec_line_and_tick_summary() {
    let exec = output::watch::render_exec(&WatchExecEvent {
        node_id: "process:pid:4421".to_string(),
        comm: "curl".to_string(),
    });
    assert_eq!(exec, "[exec] process:pid:4421 curl");

    let quiet = output::watch::render_tick(&WatchTick {
        at_ns: 0,
        baseline: false,
        processes: 2,
        connections: 0,
        listening_ports: 0,
        processes_added: 0,
        processes_removed: 0,
        connections_added: 0,
        connections_removed: 0,
        listening_ports_added: 0,
        listening_ports_removed: 0,
    });
    assert_eq!(quiet, "[00:00:00] no significant changes");

    let changed = output::watch::render_tick(&WatchTick {
        at_ns: 0,
        baseline: false,
        processes: 3,
        connections: 1,
        listening_ports: 0,
        processes_added: 2,
        processes_removed: 1,
        connections_added: 1,
        connections_removed: 0,
        listening_ports_added: 0,
        listening_ports_removed: 0,
    });
    assert_eq!(
        changed,
        "[00:00:00] +2 processes, -1 process, +1 connection"
    );

    let banner = output::watch::render_banner(5);
    assert!(banner.contains("twin watch"));
    assert!(banner.contains("ok"));
    assert!(banner.contains("every 5s"));

    let warning = output::watch::render_warning("kernel: unsupported");
    assert!(warning.contains("warn"));
    assert!(warning.contains("kernel"));
}
