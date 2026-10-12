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

    fn on_tcp(&mut self, _event: &twin_app::WatchTcpEvent) {}

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
            ebpf: WatchEbpf::Injected(vec![EbpfExec::new(4421, 4400, 1000, "curl", 9, 1)]),
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
    let store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    let events = store
        .list_observations_by_kind("ebpf_exec_observed")
        .expect("events");
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].subject_node_id.as_deref(),
        Some("process:pid:4421")
    );
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
            ebpf: WatchEbpf::Injected(vec![EbpfExec::new(1, 0, 1000, "sleep", 1, 1)]),
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
fn unknown_events_are_a_typed_error() {
    let err = parse_watch(&WatchCommand {
        config_override: None,
        interval: "5s".to_string(),
        duration: None,
        ticks: Some(1),
        ebpf: true,
        events: Some("dns".to_string()),
    })
    .expect_err("dns");
    let text = err.to_string();
    assert!(text.contains("dns"), "{text}");
    assert!(text.contains("exec"), "{text}");
    assert!(text.contains("tcp"), "{text}");
}

#[test]
fn tcp_events_are_accepted() {
    let run = parse_watch(&WatchCommand {
        config_override: None,
        interval: "5s".to_string(),
        duration: None,
        ticks: Some(1),
        ebpf: true,
        events: Some("tcp".to_string()),
    })
    .expect("tcp");
    assert!(matches!(run.ebpf, WatchEbpf::LiveTcp));
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
        tcp_tracepoint_present: true,
    };
    let result = doctor_ebpf_in(&home.layout, None, Some(facts)).expect("doctor");
    let ebpf = result.ebpf.expect("ebpf section");
    assert!(!ebpf.kernel.ok);
    assert_eq!(ebpf.kernel.status, "unsupported");
    assert!(ebpf.btf.ok);
    assert!(ebpf.capabilities.ok);
    assert!(ebpf.exec_tracing.ok);
}

#[test]
fn sustained_polling_keeps_a_bounded_tick_window_and_reports_collector_timings() {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let (home, proc_root) = layout_and_proc();
    struct Counter(u32);
    impl WatchSink for Counter {
        fn on_start(&mut self, _: u64) {}
        fn on_tick(&mut self, tick: &WatchTick) {
            self.0 += 1;
            assert!(tick.collector_timings_ms.contains_key("process"));
        }
        fn on_exec(&mut self, _: &WatchExecEvent) {}
        fn on_tcp(&mut self, _: &twin_app::WatchTcpEvent) {}
        fn on_warning(&mut self, _: &str) {}
    }
    let mut sink = Counter(0);
    let result = watch_in(
        &home.layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(260),
            ebpf: WatchEbpf::Off,
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut sink,
    )
    .unwrap();
    assert_eq!(sink.0, 260);
    assert_eq!(result.ticks_completed, 260);
    assert_eq!(result.ticks.len(), 256);
    assert!(!result.ticks[0].baseline);
    assert!(result
        .ticks
        .last()
        .unwrap()
        .collector_timings_ms
        .contains_key("scan_total"));
}

#[test]
fn exec_retains_birth_identity_and_stales_previous_runtime_relationships() {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let (home, proc_root) = layout_and_proc();
    let mut fields = vec!["0"; 50];
    fields[0] = "S";
    fields[1] = "1";
    fields[19] = "100";
    support::write_proc_fixture(
        &proc_root,
        42,
        &format!("42 (worker) {}", fields.join(" ")),
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"worker\0",
        None,
    );
    std::fs::create_dir_all(proc_root.join("42/fd")).unwrap();
    for table in ["tcp", "tcp6", "udp", "udp6"] {
        support::write_tcp_table(&proc_root, table, "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n");
    }
    support::write_unix_table(
        &proc_root,
        "Num RefCount Protocol Flags Type St Inode Path\n",
    );
    let mut store = twin_store::Store::open(&home.layout.db_file()).unwrap();
    let mut ingest = twin_app::TcpIngestor::new(twin_ebpf::TcpRateLimits::session());
    ingest
        .push(
            &mut store,
            &twin_ebpf::TcpEvent::Connect(twin_ebpf::TcpConnect::new(
                42,
                9,
                "127.0.0.1".parse().unwrap(),
                "192.0.2.1".parse().unwrap(),
                443,
                1,
            )),
        )
        .unwrap();
    let mut bytes = twin_ebpf::encode_exec(&EbpfExec::new(
        42,
        1,
        1000,
        "worker",
        9,
        twin_core::TimestampNs::now().as_i64() as u64,
    ))
    .to_vec();
    bytes.extend_from_slice(&2u64.to_le_bytes());
    bytes.extend_from_slice(&100u64.to_le_bytes());
    let event = twin_ebpf::decode_exec(&bytes).unwrap();
    let mut rec = Rec {
        ticks: vec![],
        execs: vec![],
        warnings: vec![],
    };
    watch_in(
        &home.layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(1),
            ebpf: WatchEbpf::Injected(vec![event]),
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut rec,
    )
    .unwrap();
    let node = store.get_node("process:pid:42").unwrap().unwrap();
    let metadata: serde_json::Value = serde_json::from_str(&node.metadata_json).unwrap();
    assert_eq!(metadata["start_ticks"], 100);
    assert!(metadata["last_exec_ns"].as_i64().unwrap() > 1);
    let edge = store
        .get_edge("process:pid:42|connects_to|port:tcp:192.0.2.1:443")
        .unwrap()
        .unwrap();
    assert_eq!(edge.state, "stale");
}
