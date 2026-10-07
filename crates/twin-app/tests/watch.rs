mod support;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use twin_app::{InitRequest, WatchRequest, WatchStop};
use twin_store::Store;

use support::{write_proc_fixture, write_socket_fd, write_tcp_table, IsolatedHome};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";
const SYSTEMD_STAT: &str = "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";
const WORKER_STAT: &str = "42 (worker) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0";

fn with_scan_env<T>(body: impl FnOnce() -> T) -> T {
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let result = body();
    support::clear_scan_env();
    result
}

fn write_pair(proc_root: &Path) {
    write_proc_fixture(
        proc_root,
        1,
        SYSTEMD_STAT,
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/lib/systemd/systemd\0",
        Some(Path::new("/usr/lib/systemd/systemd")),
    );
    write_proc_fixture(
        proc_root,
        42,
        WORKER_STAT,
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/worker\0",
        Some(Path::new("/usr/bin/worker")),
    );
}

fn write_listener_and_connection(proc_root: &Path) {
    write_pair(proc_root);
    write_tcp_table(
        proc_root,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(proc_root, 42, 8, 12345);
    write_socket_fd(proc_root, 1, 3, 456);
}

#[test]
fn watch_requires_initialized_database() {
    let home = IsolatedHome::new();
    let proc_root = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let error = twin_app::watch_in(
        &home.layout,
        WatchRequest {
            interval: "0s".to_string(),
            max_ticks: Some(1),
            ..WatchRequest::default()
        },
        &proc_root,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect_err("missing db");
    assert!(error.to_string().contains("not initialized"), "{error}");
}

#[test]
fn watch_rejects_bad_interval_duration_and_tick_count() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc_root = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let interval = twin_app::watch_in(
        &home.layout,
        WatchRequest {
            interval: "nope".to_string(),
            max_ticks: Some(1),
            ..WatchRequest::default()
        },
        &proc_root,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect_err("interval");
    assert!(
        interval.to_string().contains("invalid interval"),
        "{interval}"
    );

    let duration = twin_app::watch_in(
        &home.layout,
        WatchRequest {
            interval: "0s".to_string(),
            duration: Some("0s".to_string()),
            max_ticks: Some(1),
            ..WatchRequest::default()
        },
        &proc_root,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect_err("duration");
    assert!(
        duration.to_string().contains("invalid duration"),
        "{duration}"
    );

    let ticks = twin_app::watch_in(
        &home.layout,
        WatchRequest {
            interval: "0s".to_string(),
            max_ticks: Some(0),
            ..WatchRequest::default()
        },
        &proc_root,
        &AtomicBool::new(false),
        |_| {},
    )
    .expect_err("ticks");
    assert!(ticks.to_string().contains("tick count"), "{ticks}");
}

#[test]
fn watch_reports_deltas_then_stays_quiet_and_keeps_graph_stable() {
    with_scan_env(|| {
        let home = IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
        let proc_root = home.layout.data_dir.join("proc");
        std::fs::create_dir_all(&proc_root).expect("proc");
        write_listener_and_connection(&proc_root);

        let mut ticks = Vec::new();
        let mut rss = Vec::new();
        let result = twin_app::watch_in(
            &home.layout,
            WatchRequest {
                interval: "0s".to_string(),
                max_ticks: Some(2),
                ..WatchRequest::default()
            },
            &proc_root,
            &AtomicBool::new(false),
            |tick| {
                if tick.index == 1 {
                    std::fs::remove_dir_all(proc_root.join("42")).expect("drop worker");
                    let _ = std::fs::remove_file(proc_root.join("1").join("fd").join("3"));
                    write_tcp_table(&proc_root, "tcp", TCP_HEADER);
                }
                ticks.push(tick.clone());
            },
        )
        .expect("watch");

        assert_eq!(result.ticks, 2);
        assert_eq!(result.stop, WatchStop::MaxTicks);
        assert_eq!(ticks.len(), 2);
        assert_eq!(ticks[0].processes_added, 2, "{:?}", ticks[0]);
        assert!(
            ticks[0].listening_ports_added >= 1,
            "expected a new listening port: {:?}",
            ticks[0]
        );
        assert!(
            ticks[0].connections_added >= 1,
            "expected a new connection: {:?}",
            ticks[0]
        );
        assert!(
            ticks[1].processes_removed >= 1,
            "expected a removed process: {:?}",
            ticks[1]
        );
        assert!(
            ticks[1].listening_ports_removed + ticks[1].listening_ports_stale >= 1,
            "expected listening port to leave: {:?}",
            ticks[1]
        );
        assert!(
            ticks[0]
                .collector_timings
                .iter()
                .any(|timing| timing.collector == "proc_process"),
            "{:?}",
            ticks[0].collector_timings
        );
        assert_eq!(
            result.event_count,
            ticks[0].event_count + ticks[1].event_count
        );

        let store = Store::open(&home.layout.db_file()).expect("store");
        let processes = store.list_nodes_by_kind("process").expect("processes");
        assert_eq!(processes.len(), 1, "active processes: {processes:?}");

        let stable = twin_app::watch_in(
            &home.layout,
            WatchRequest {
                interval: "0s".to_string(),
                max_ticks: Some(8),
                ..WatchRequest::default()
            },
            &proc_root,
            &AtomicBool::new(false),
            |tick| {
                if tick.index == 2 || tick.index == 8 {
                    rss.push(rss_kib());
                }
                assert_eq!(tick.event_count, 0, "rescan should be quiet: {tick:?}");
            },
        )
        .expect("stable watch");
        assert_eq!(stable.ticks, 8);
        assert_eq!(stable.event_count, 0);
        assert!(
            stable
                .collector_timings
                .iter()
                .any(|timing| timing.collector == "proc_process" && timing.runs >= 8),
            "{:?}",
            stable.collector_timings
        );
        let processes = store.list_nodes_by_kind("process").expect("processes");
        assert_eq!(processes.len(), 1);
        assert_eq!(rss.len(), 2, "rss samples");
        assert!(
            rss[1] < rss[0].saturating_add(16 * 1024),
            "rss grew from {} KiB to {} KiB across quiet scans",
            rss[0],
            rss[1]
        );
    });
}

#[test]
fn watch_stops_when_the_flag_is_set_and_when_duration_elapses() {
    with_scan_env(|| {
        let home = IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
        let proc_root = home.layout.data_dir.join("proc");
        std::fs::create_dir_all(&proc_root).expect("proc");
        write_pair(&proc_root);

        let stop = AtomicBool::new(false);
        let interrupted = twin_app::watch_in(
            &home.layout,
            WatchRequest {
                interval: "30s".to_string(),
                ..WatchRequest::default()
            },
            &proc_root,
            &stop,
            |_| {
                stop.store(true, Ordering::SeqCst);
            },
        )
        .expect("interrupt");
        assert_eq!(interrupted.ticks, 1);
        assert_eq!(interrupted.stop, WatchStop::Interrupted);

        let timed = twin_app::watch_in(
            &home.layout,
            WatchRequest {
                interval: "30s".to_string(),
                duration: Some("1s".to_string()),
                ..WatchRequest::default()
            },
            &proc_root,
            &AtomicBool::new(false),
            |_| {},
        )
        .expect("duration");
        assert_eq!(timed.ticks, 1, "duration should stop before a second scan");
        assert_eq!(timed.stop, WatchStop::Duration);
    });
}

fn rss_kib() -> u64 {
    let status = std::fs::read_to_string("/proc/self/status").expect("status");
    for line in status.lines() {
        if let Some(rest) = line.strip_prefix("VmRSS:") {
            return rest
                .split_whitespace()
                .next()
                .expect("rss")
                .parse()
                .expect("rss number");
        }
    }
    panic!("VmRSS missing");
}
