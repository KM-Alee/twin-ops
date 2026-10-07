use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use twin_core::NodeKind;
use twin_ebpf::{EbpfExec, TcpEvent, TcpRateLimits};
use twin_observation::Pipeline;
use twin_store::Store;

use crate::commands::ebpf_tcp::TcpIngestor;
use crate::error::{AppError, ScanError, WatchError};
use crate::model::{WatchExecEvent, WatchResult, WatchTcpEvent, WatchTick};
use crate::paths::TwinLayout;
use crate::ScanRequest;

use super::duration::clock_duration;

pub trait WatchSink {
    fn on_start(&mut self, interval_secs: u64);
    fn on_tick(&mut self, tick: &WatchTick);
    fn on_exec(&mut self, event: &WatchExecEvent);
    fn on_tcp(&mut self, event: &WatchTcpEvent);
    fn on_warning(&mut self, message: &str);
}

#[derive(Debug, Clone)]
pub struct WatchCommand {
    pub config_override: Option<PathBuf>,
    pub interval: String,
    pub duration: Option<String>,
    pub ticks: Option<u32>,
    pub ebpf: bool,
    pub events: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WatchRun {
    pub config_override: Option<PathBuf>,
    pub interval: Duration,
    pub duration: Option<Duration>,
    pub max_ticks: Option<u32>,
    pub ebpf: WatchEbpf,
}

enum WatchStream {
    Exec,
    Tcp,
}

#[derive(Debug, Clone)]
pub enum WatchEbpf {
    Off,
    Live,
    LiveTcp,
    Injected(Vec<EbpfExec>),
    InjectedTcp(Vec<TcpEvent>),
    Unavailable { reason: String },
}

pub fn prepare(command: &WatchCommand) -> Result<WatchRun, AppError> {
    let stream = match command.events.as_deref() {
        None | Some("exec") => WatchStream::Exec,
        Some("tcp") => WatchStream::Tcp,
        Some(name) => {
            return Err(WatchError::UnsupportedEvent {
                name: name.to_string(),
            }
            .into());
        }
    };
    if let Some(name) = command.events.as_deref() {
        if !command.ebpf {
            return Err(WatchError::EventsRequireEbpf {
                name: name.to_string(),
            }
            .into());
        }
    }
    if let Some(0) = command.ticks {
        return Err(WatchError::InvalidTicks {
            value: 0,
            reason: "tick count must be at least 1".to_string(),
        }
        .into());
    }
    let interval =
        clock_duration(&command.interval).map_err(|reason| WatchError::InvalidInterval {
            value: command.interval.clone(),
            reason,
        })?;
    let duration = match &command.duration {
        Some(value) => {
            Some(
                clock_duration(value).map_err(|reason| WatchError::InvalidDuration {
                    value: value.clone(),
                    reason,
                })?,
            )
        }
        None => None,
    };
    let ebpf = if !command.ebpf {
        WatchEbpf::Off
    } else if matches!(stream, WatchStream::Tcp) {
        WatchEbpf::LiveTcp
    } else {
        WatchEbpf::Live
    };
    Ok(WatchRun {
        config_override: command.config_override.clone(),
        interval,
        duration,
        max_ticks: command.ticks,
        ebpf,
    })
}

pub fn run<S: WatchSink>(
    layout: &TwinLayout,
    run: WatchRun,
    proc_root: &Path,
    stop: &AtomicBool,
    sink: &mut S,
) -> Result<WatchResult, AppError> {
    ensure_database(layout)?;
    let interval_secs = run.interval.as_secs();
    sink.on_start(interval_secs);
    let started = Instant::now();
    let mut warnings = Vec::new();
    let mut exec_count = 0u64;
    let mut tcp_count = 0u64;
    let mut ticks = Vec::new();
    let mut previous: Option<TickSnapshot> = None;
    let (mut stream, ebpf_attached) = open_stream(&run.ebpf, sink, &mut warnings);
    let mut tcp_ingest = TcpIngestor::new(TcpRateLimits::session());

    while !stop.load(Ordering::SeqCst) && !limit_reached(&run, started, ticks.len() as u32) {
        let scan = super::scan::run(
            layout,
            &ScanRequest {
                config_override: run.config_override.clone(),
                samples: 1,
                interval_secs: 1,
            },
            proc_root,
        )?;
        let snapshot = TickSnapshot {
            process_ids: active_process_ids(&layout.db_file())?,
            connections: scan.tcp_connection_count as u64,
            listening_ports: scan.tcp_listener_count as u64,
            processes: scan.process_count as u64,
        };
        let tick = make_tick(scan.ended_at_ns, &previous, &snapshot);
        previous = Some(snapshot);
        sink.on_tick(&tick);
        ticks.push(tick);
        if let Some(open) = stream.as_mut() {
            let drained = match open {
                OpenStream::Exec(exec) => drain_exec(exec, sink, &mut exec_count, &mut warnings),
                OpenStream::Tcp(tcp) => drain_tcp(
                    tcp,
                    layout,
                    sink,
                    &mut tcp_ingest,
                    &mut tcp_count,
                    &mut warnings,
                ),
            };
            if matches!(drained, Drain::StopStream) {
                stream.take();
            }
        }
        if stop.load(Ordering::SeqCst) || limit_reached(&run, started, ticks.len() as u32) {
            break;
        }
        if wait_interval(run.interval, run.duration, started, stop) {
            break;
        }
    }

    drop(stream);
    Ok(WatchResult {
        interval_secs,
        duration_secs: run.duration.map(|duration| duration.as_secs()),
        ticks_completed: ticks.len() as u32,
        exec_count,
        tcp_count,
        ebpf_requested: !matches!(run.ebpf, WatchEbpf::Off),
        ebpf_attached,
        warnings,
        ticks,
    })
}

struct TickSnapshot {
    process_ids: HashSet<String>,
    connections: u64,
    listening_ports: u64,
    processes: u64,
}

enum OpenStream {
    Exec(OpenExec),
    Tcp(OpenTcp),
}

enum OpenExec {
    Session(Box<twin_ebpf::ExecSession>),
    Injected(std::vec::IntoIter<EbpfExec>),
}

enum OpenTcp {
    Session(Box<twin_ebpf::TcpSession>),
    Injected(std::vec::IntoIter<TcpEvent>),
}

enum Drain {
    Continue,
    StopStream,
}

fn open_stream<S: WatchSink>(
    mode: &WatchEbpf,
    sink: &mut S,
    warnings: &mut Vec<String>,
) -> (Option<OpenStream>, bool) {
    match mode {
        WatchEbpf::Off => (None, false),
        WatchEbpf::Unavailable { reason } => {
            note(sink, warnings, reason.clone());
            (None, false)
        }
        WatchEbpf::Injected(events) => (
            Some(OpenStream::Exec(OpenExec::Injected(
                events.clone().into_iter(),
            ))),
            true,
        ),
        WatchEbpf::InjectedTcp(events) => (
            Some(OpenStream::Tcp(OpenTcp::Injected(
                events.clone().into_iter(),
            ))),
            true,
        ),
        WatchEbpf::Live => match live_exec_session() {
            Ok(session) => (
                Some(OpenStream::Exec(OpenExec::Session(Box::new(session)))),
                true,
            ),
            Err(reason) => {
                note(sink, warnings, reason);
                (None, false)
            }
        },
        WatchEbpf::LiveTcp => match live_tcp_session() {
            Ok(session) => (
                Some(OpenStream::Tcp(OpenTcp::Session(Box::new(session)))),
                true,
            ),
            Err(reason) => {
                note(sink, warnings, reason);
                (None, false)
            }
        },
    }
}

fn live_exec_session() -> Result<twin_ebpf::ExecSession, String> {
    let report = twin_ebpf::assess(&twin_ebpf::host_facts());
    if !report.is_ready() {
        return Err(report.failure_summary());
    }
    twin_ebpf::attach().map_err(|err| err.to_string())
}

fn live_tcp_session() -> Result<twin_ebpf::TcpSession, String> {
    let report = twin_ebpf::assess(&twin_ebpf::host_facts());
    if !report.is_tcp_ready() {
        return Err(report.tcp_failure_summary());
    }
    twin_ebpf::attach_tcp().map_err(|err| err.to_string())
}

fn drain_exec<S: WatchSink>(
    open: &mut OpenExec,
    sink: &mut S,
    exec_count: &mut u64,
    warnings: &mut Vec<String>,
) -> Drain {
    let pipeline = Pipeline::default();
    loop {
        let event = match poll_exec(open) {
            Ok(Some(event)) => event,
            Ok(None) => return Drain::Continue,
            Err(err) => {
                note(sink, warnings, err.to_string());
                return Drain::StopStream;
            }
        };
        let raw = twin_ebpf::exec_to_raw(&event);
        match pipeline.process(raw) {
            Ok(observation) => {
                let node_id = observation
                    .subject()
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| format!("process:pid:{}", event.pid()));
                let comm = observation
                    .metadata()
                    .get("comm")
                    .and_then(|value| value.as_str())
                    .unwrap_or("")
                    .to_string();
                *exec_count += 1;
                sink.on_exec(&WatchExecEvent { node_id, comm });
            }
            Err(err) => note(sink, warnings, err.to_string()),
        }
    }
}

fn drain_tcp<S: WatchSink>(
    open: &mut OpenTcp,
    layout: &TwinLayout,
    sink: &mut S,
    ingest: &mut TcpIngestor,
    tcp_count: &mut u64,
    warnings: &mut Vec<String>,
) -> Drain {
    let mut store = match Store::open(&layout.db_file()) {
        Ok(store) => store,
        Err(err) => {
            note(sink, warnings, err.to_string());
            return Drain::StopStream;
        }
    };
    let mut last_ts = 0i64;
    loop {
        let event = match poll_tcp(open) {
            Ok(Some(event)) => event,
            Ok(None) => break,
            Err(err) => {
                note(sink, warnings, err.to_string());
                return Drain::StopStream;
            }
        };
        last_ts = i64::try_from(event.timestamp_ns()).unwrap_or(last_ts);
        match ingest.push(&mut store, &event) {
            Ok(Some(line)) => {
                *tcp_count += 1;
                sink.on_tcp(&line);
            }
            Ok(None) => {}
            Err(err) => note(sink, warnings, err.to_string()),
        }
    }
    if let Err(err) = ingest.sync_drops(&mut store, last_ts) {
        note(sink, warnings, err.to_string());
    }
    Drain::Continue
}

fn poll_tcp(open: &mut OpenTcp) -> Result<Option<TcpEvent>, twin_ebpf::EbpfError> {
    match open {
        OpenTcp::Session(session) => session.poll_event(),
        OpenTcp::Injected(events) => Ok(events.next()),
    }
}

fn poll_exec(open: &mut OpenExec) -> Result<Option<EbpfExec>, twin_ebpf::EbpfError> {
    match open {
        OpenExec::Session(session) => session.poll_exec(),
        OpenExec::Injected(events) => Ok(events.next()),
    }
}

fn make_tick(at_ns: i64, previous: &Option<TickSnapshot>, current: &TickSnapshot) -> WatchTick {
    let Some(previous) = previous else {
        return WatchTick {
            at_ns,
            baseline: true,
            processes: current.processes,
            connections: current.connections,
            listening_ports: current.listening_ports,
            processes_added: 0,
            processes_removed: 0,
            connections_added: 0,
            connections_removed: 0,
            listening_ports_added: 0,
            listening_ports_removed: 0,
        };
    };
    let processes_added = current
        .process_ids
        .difference(&previous.process_ids)
        .count() as u64;
    let processes_removed = previous
        .process_ids
        .difference(&current.process_ids)
        .count() as u64;
    let (connections_added, connections_removed) =
        split_delta(previous.connections, current.connections);
    let (listening_ports_added, listening_ports_removed) =
        split_delta(previous.listening_ports, current.listening_ports);
    WatchTick {
        at_ns,
        baseline: false,
        processes: current.processes,
        connections: current.connections,
        listening_ports: current.listening_ports,
        processes_added,
        processes_removed,
        connections_added,
        connections_removed,
        listening_ports_added,
        listening_ports_removed,
    }
}

fn split_delta(previous: u64, current: u64) -> (u64, u64) {
    if current >= previous {
        (current - previous, 0)
    } else {
        (0, previous - current)
    }
}

fn ensure_database(layout: &TwinLayout) -> Result<(), AppError> {
    let db = layout.db_file();
    if !db.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let store = Store::open(&db).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    Ok(())
}

fn active_process_ids(db: &Path) -> Result<HashSet<String>, AppError> {
    let store = Store::open(db).map_err(ScanError::StoreOpen)?;
    let rows = store
        .list_nodes_by_kind(&NodeKind::Process.to_string())
        .map_err(ScanError::Store)?;
    Ok(rows.into_iter().map(|row| row.id).collect())
}

fn limit_reached(run: &WatchRun, started: Instant, ticks: u32) -> bool {
    if let Some(max) = run.max_ticks {
        if ticks >= max {
            return true;
        }
    }
    if let Some(duration) = run.duration {
        if started.elapsed() >= duration {
            return true;
        }
    }
    false
}

fn wait_interval(
    interval: Duration,
    limit: Option<Duration>,
    started: Instant,
    stop: &AtomicBool,
) -> bool {
    if interval.is_zero() {
        return stop.load(Ordering::SeqCst);
    }
    let step = Duration::from_millis(50);
    let deadline = Instant::now() + interval;
    while Instant::now() < deadline {
        if stop.load(Ordering::SeqCst) {
            return true;
        }
        if let Some(limit) = limit {
            if started.elapsed() >= limit {
                return true;
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            break;
        }
        thread::sleep(step.min(remaining));
    }
    stop.load(Ordering::SeqCst)
}

fn note<S: WatchSink>(sink: &mut S, warnings: &mut Vec<String>, message: String) {
    sink.on_warning(&message);
    warnings.push(message);
}
