mod support;

use support::{stderr_utf8, stdout_utf8, TwinHome};

#[test]
fn doctor_ebpf_exits_cleanly_and_lists_checks() {
    let home = TwinHome::new();
    let out = home.run_without_proc(&["doctor", "--ebpf"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("eBPF"));
    assert!(text.contains("kernel"));
    assert!(text.contains("BTF"));
    assert!(text.contains("capabilities"));
    assert!(text.contains("exec tracing"));
}

#[test]
fn doctor_ebpf_json_includes_four_checks() {
    let home = TwinHome::new();
    let out = home.run_without_proc(&["--json", "doctor", "--ebpf"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let value: serde_json::Value = serde_json::from_str(stdout_utf8(&out).trim()).expect("json");
    for key in ["kernel", "btf", "capabilities", "exec_tracing"] {
        assert!(value["ebpf"][key]["status"].is_string(), "{key}");
        assert!(value["ebpf"][key]["ok"].is_boolean(), "{key}");
    }
}

#[test]
fn watch_without_ebpf_prints_a_tick_and_stops() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let out = home.run(&["watch", "--interval", "1s", "--ticks", "1"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("twin watch"));
    assert!(text.contains('['), "{text}");
    assert!(!text.contains("[exec]"));
}

#[test]
fn watch_ebpf_warns_and_still_prints_a_tick() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let out = home.run(&[
        "watch",
        "--ebpf",
        "--events",
        "exec",
        "--interval",
        "1s",
        "--ticks",
        "1",
    ]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("warn"), "{text}");
    assert!(text.contains('['), "{text}");
}

#[test]
fn watch_rejects_tcp_events() {
    let home = TwinHome::new();
    let out = home.run(&["watch", "--ebpf", "--events", "tcp", "--ticks", "1"]);
    assert!(!out.status.success());
    let err = stderr_utf8(&out);
    assert!(err.contains("tcp"), "{err}");
}

#[test]
fn watch_without_init_fails_cleanly() {
    let home = TwinHome::new();
    let out = home.run(&["watch", "--ticks", "1", "--interval", "1s"]);
    assert!(!out.status.success());
    assert!(stderr_utf8(&out).contains("not initialized"));
}
