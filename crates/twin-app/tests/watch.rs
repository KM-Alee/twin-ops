mod support;

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use twin_app::{
    doctor_ebpf_in, init_in, parse_watch, watch_in, InitRequest, WatchCommand, WatchEbpf,
    WatchExecEvent, WatchRun, WatchSink, WatchTick,
};
use twin_ebpf::{EbpfExec, EbpfFacts};

struct Rec {
    ticks: Vec<WatchTick>,
    execs: Vec<WatchExecEvent>,
    warnings: Vec<String>,
}

impl WatchSink for Rec {
    fn on_start(&mut self, _interval_secs: u64) {}

    fn on_tick(&mut self, tick: &WatchTick) {
        self.ticks.push(tick.clone());
    }

    fn on_exec(&mut self, event: &WatchExecEvent) {
        self.execs.push(event.clone());
    }

    fn on_warning(&mut self, message: &str) {
        self.warnings.push(message.to_string());
    }
}

fn layout_and_proc() -> (support::IsolatedHome, std::path::PathBuf) {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let proc_root = home.layout.db_file().parent().expect("parent").join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    (home, proc_root)
}

#[test]
fn injected_exec_is_observed_and_ticks_stop() {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let (home, proc_root) = layout_and_proc();
    let mut rec = Rec {
        ticks: Vec::new(),
        execs: Vec::new(),
        warnings: Vec::new(),
    };
    let result = watch_in(
        &home.layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(1),
            ebpf: WatchEbpf::Injected(vec![EbpfExec::new(4421, 4400, "curl", 9, 1)]),
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut rec,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 1);
    assert_eq!(result.exec_count, 1);
    assert_eq!(rec.execs.len(), 1);
    assert_eq!(rec.execs[0].node_id, "process:pid:4421");
    assert_eq!(rec.execs[0].comm, "curl");
    assert!(result.warnings.is_empty());
}

#[test]
fn unavailable_ebpf_warns_and_still_ticks() {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let (home, proc_root) = layout_and_proc();
    let mut rec = Rec {
        ticks: Vec::new(),
        execs: Vec::new(),
        warnings: Vec::new(),
    };
    let result = watch_in(
        &home.layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(2),
            ebpf: WatchEbpf::Unavailable {
                reason: "kernel: unsupported (5.4.0 is older than 5.8)".to_string(),
            },
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut rec,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 2);
    assert!(!result.ebpf_attached);
    assert!(result.ebpf_requested);
    assert_eq!(result.exec_count, 0);
    assert!(rec
        .warnings
        .iter()
        .any(|warning| warning.contains("kernel")));
    assert!(rec.ticks[0].baseline);
    assert_eq!(rec.ticks[1].processes_added, 0);
    assert_eq!(rec.ticks[1].processes_removed, 0);
}

#[test]
fn stop_flag_ends_the_loop_before_a_tick() {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let (home, proc_root) = layout_and_proc();
    let mut rec = Rec {
        ticks: Vec::new(),
        execs: Vec::new(),
        warnings: Vec::new(),
    };
    let stop = AtomicBool::new(true);
    let result = watch_in(
        &home.layout,
        WatchRun {
            config_override: None,
            interval: Duration::from_secs(30),
            duration: None,
            max_ticks: Some(5),
            ebpf: WatchEbpf::Injected(vec![EbpfExec::new(1, 0, "sleep", 1, 1)]),
        },
        &proc_root,
        &stop,
        &mut rec,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 0);
    assert!(rec.execs.is_empty());
}

#[test]
fn tcp_events_are_a_typed_error() {
    let err = parse_watch(&WatchCommand {
        config_override: None,
        interval: "5s".to_string(),
        duration: None,
        ticks: Some(1),
        ebpf: true,
        events: Some("tcp".to_string()),
    })
    .expect_err("tcp");
    let text = err.to_string();
    assert!(text.contains("tcp"), "{text}");
    assert!(text.contains("exec"), "{text}");
}

#[test]
fn events_require_ebpf_and_zero_ticks_are_rejected() {
    let err = parse_watch(&WatchCommand {
        config_override: None,
        interval: "5s".to_string(),
        duration: None,
        ticks: Some(1),
        ebpf: false,
        events: Some("exec".to_string()),
    })
    .expect_err("events");
    assert!(err.to_string().contains("--ebpf"));

    let err = parse_watch(&WatchCommand {
        config_override: None,
        interval: "0s".to_string(),
        duration: None,
        ticks: Some(1),
        ebpf: false,
        events: None,
    })
    .expect_err("interval");
    assert!(err.to_string().contains("interval"));

    let err = parse_watch(&WatchCommand {
        config_override: None,
        interval: "5s".to_string(),
        duration: None,
        ticks: Some(0),
        ebpf: false,
        events: None,
    })
    .expect_err("ticks");
    assert!(err.to_string().contains("tick"));
}

#[test]
fn doctor_ebpf_names_the_failing_check() {
    let home = support::IsolatedHome::new();
    let facts = EbpfFacts {
        kernel_release: "5.4.0".to_string(),
        btf_present: true,
        effective_caps: Some(1u64 << 21),
        exec_tracepoint_present: true,
    };
    let result = doctor_ebpf_in(&home.layout, None, Some(facts)).expect("doctor");
    let ebpf = result.ebpf.expect("ebpf section");
    assert!(!ebpf.kernel.ok);
    assert_eq!(ebpf.kernel.status, "unsupported");
    assert!(ebpf.btf.ok);
    assert!(ebpf.capabilities.ok);
    assert!(ebpf.exec_tracing.ok);
}
