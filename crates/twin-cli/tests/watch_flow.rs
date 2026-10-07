use std::sync::atomic::AtomicBool;
use std::time::Duration;

use twin_app::paths::TwinLayout;
use twin_app::{
    init_in, watch_in, InitRequest, WatchEbpf, WatchExecEvent, WatchRun, WatchSink, WatchTick,
};
use twin_cli::output;
use twin_ebpf::{EbpfExec, TcpConnect, TcpEvent};

struct Lines {
    text: Vec<String>,
}

impl WatchSink for Lines {
    fn on_start(&mut self, interval_secs: u64) {
        self.text.push(output::watch::render_banner(interval_secs));
    }

    fn on_tick(&mut self, tick: &WatchTick) {
        self.text.push(output::watch::render_tick(tick));
    }

    fn on_exec(&mut self, event: &WatchExecEvent) {
        self.text.push(output::watch::render_exec(event));
    }

    fn on_tcp(&mut self, event: &twin_app::WatchTcpEvent) {
        self.text.push(output::watch::render_tcp(event));
    }

    fn on_warning(&mut self, message: &str) {
        self.text.push(output::watch::render_warning(message));
    }
}

#[test]
fn injected_exec_prints_and_stops_on_ticks() {
    let temp = tempfile::tempdir().expect("temp");
    let layout = TwinLayout::isolated(temp.path());
    init_in(&layout, InitRequest::default()).expect("init");
    let proc_root = temp.path().join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let mut lines = Lines { text: Vec::new() };
    let result = watch_in(
        &layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(1),
            ebpf: WatchEbpf::Injected(vec![EbpfExec::new(4421, 1, "curl", 3, 1)]),
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut lines,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 1);
    assert_eq!(result.exec_count, 1);
    let rendered = lines.text.join("\n");
    assert!(
        rendered.contains("[exec] process:pid:4421 curl"),
        "{rendered}"
    );
    assert!(rendered.contains("twin watch"));
    let json = output::json::render(&result).expect("json");
    assert!(json.contains("\"exec_count\": 1"));
    assert!(!json.contains("curl"));
}

#[test]
fn injected_tcp_prints_beside_the_tick_and_stops() {
    let temp = tempfile::tempdir().expect("temp");
    let layout = TwinLayout::isolated(temp.path());
    init_in(&layout, InitRequest::default()).expect("init");
    let proc_root = temp.path().join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let mut lines = Lines { text: Vec::new() };
    let event = TcpEvent::Connect(TcpConnect::new(
        4421,
        4,
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 1)),
        std::net::IpAddr::V4(std::net::Ipv4Addr::new(10, 0, 0, 8)),
        5432,
        1,
    ));
    let result = watch_in(
        &layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(1),
            ebpf: WatchEbpf::InjectedTcp(vec![event]),
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut lines,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 1);
    assert_eq!(result.tcp_count, 1);
    assert_eq!(result.exec_count, 0);
    let rendered = lines.text.join("\n");
    assert!(
        rendered.contains("[tcp] process:pid:4421 connect 10.0.0.8:5432"),
        "{rendered}"
    );
    assert!(
        rendered.contains("baseline") || rendered.contains('['),
        "{rendered}"
    );
}

fn unavailable_ebpf_warning_keeps_polling_ticks() {
    let temp = tempfile::tempdir().expect("temp");
    let layout = TwinLayout::isolated(temp.path());
    init_in(&layout, InitRequest::default()).expect("init");
    let proc_root = temp.path().join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let mut lines = Lines { text: Vec::new() };
    let result = watch_in(
        &layout,
        WatchRun {
            config_override: None,
            interval: Duration::ZERO,
            duration: None,
            max_ticks: Some(1),
            ebpf: WatchEbpf::Unavailable {
                reason: "capabilities: unavailable".to_string(),
            },
        },
        &proc_root,
        &AtomicBool::new(false),
        &mut lines,
    )
    .expect("watch");
    assert_eq!(result.ticks_completed, 1);
    let rendered = lines.text.join("\n");
    assert!(rendered.contains("warn"), "{rendered}");
    assert!(rendered.contains("capabilities"), "{rendered}");
    assert!(rendered.contains('['), "{rendered}");
    assert!(!rendered.contains("[exec]"));
}
