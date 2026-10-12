use std::collections::{HashSet, VecDeque};
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
    let mut ticks = VecDeque::with_capacity(256);
    let mut ticks_completed = 0u32;
    let mut previous: Option<TickSnapshot> = None;
    let (mut stream, ebpf_attached) = open_stream(&run.ebpf, sink, &mut warnings);
    let mut tcp_ingest = TcpIngestor::new(TcpRateLimits::session());

    while !stop.load(Ordering::SeqCst) && !limit_reached(&run, started, ticks_completed) {
        let scan_started = Instant::now();
        let scan = super::scan::run(
            layout,
            &ScanRequest {
                docker_fixture: None,
                containerd_fixture: None,
                config_override: run.config_override.clone(),
                samples: 1,
                interval_secs: 1,
            },
            proc_root,
        )?;
        let snapshot = TickSnapshot {
            process_ids: active_process_ids(&layout.db_file())?,
            connections: (scan.tcp_connection_count + scan.udp_connection_count) as u64,
            listening_ports: (scan.tcp_listener_count + scan.udp_listener_count) as u64,
            processes: scan.process_count as u64,
        };
        let mut tick = make_tick(scan.ended_at_ns, &previous, &snapshot);
        tick.collector_timings_ms = scan.collector_timings_ms;
        tick.collector_timings_ms.insert(
            "scan_total".into(),
            u64::try_from(scan_started.elapsed().as_millis()).unwrap_or(u64::MAX),
        );
        previous = Some(snapshot);
        sink.on_tick(&tick);
        if ticks.len() == 256 {
            ticks.pop_front();
        }
        ticks.push_back(tick);
        ticks_completed = ticks_completed.saturating_add(1);
        if let Some(open) = stream.as_mut() {
            let drained = match open {
                OpenStream::Exec(exec) => {
                    drain_exec(exec, layout, sink, &mut exec_count, &mut warnings)
                }
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
        if stop.load(Ordering::SeqCst) || limit_reached(&run, started, ticks_completed) {
            break;
        }
        if wait_interval(run.interval, run.duration, started, stop) {
            break;
        }
    }

    if let Some(open) = stream.as_mut().filter(|open| {
        matches!(
            open,
            OpenStream::Exec(OpenExec::Session(_)) | OpenStream::Tcp(OpenTcp::Session(_))
        )
    }) {
        match open {
            OpenStream::Exec(OpenExec::Session(session)) => session.stop_tracing(),
            OpenStream::Tcp(OpenTcp::Session(session)) => session.stop_tracing(),
            _ => {}
        }
        match open {
            OpenStream::Exec(exec) => {
                drain_exec(exec, layout, sink, &mut exec_count, &mut warnings);
            }
            OpenStream::Tcp(tcp) => {
                drain_tcp(
                    tcp,
                    layout,
                    sink,
                    &mut tcp_ingest,
                    &mut tcp_count,
                    &mut warnings,
                );
            }
        }
    }
    drop(stream);
    Ok(WatchResult {
        interval_secs,
        duration_secs: run.duration.map(|duration| duration.as_secs()),
        ticks_completed,
        exec_count,
        tcp_count,
        ebpf_requested: !matches!(run.ebpf, WatchEbpf::Off),
        ebpf_attached,
        warnings,
        ticks: ticks.into_iter().collect(),
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
    layout: &TwinLayout,
    sink: &mut S,
    exec_count: &mut u64,
    warnings: &mut Vec<String>,
) -> Drain {
    let pipeline = Pipeline::default();
    let mut store = match Store::open(&layout.db_file()) {
        Ok(store) => store,
        Err(error) => {
            note(sink, warnings, error.to_string());
            return Drain::StopStream;
        }
    };
    for _ in 0..32768 {
        let event = match poll_exec(open) {
            Ok(Some(event)) => event,
            Ok(None) => {
                if let OpenExec::Session(session) = open {
                    match session.dropped_count() {
                        Ok(count) if count > 0 => {
                            if let Err(error) = persist_exec_drops(&mut store, count) {
                                note(sink, warnings, error.to_string());
                            }
                        }
                        Err(error) => note(sink, warnings, error.to_string()),
                        _ => {}
                    }
                }
                return Drain::Continue;
            }
            Err(err) => {
                note(sink, warnings, err.to_string());
                return Drain::StopStream;
            }
        };
        let raw = twin_ebpf::exec_to_raw(&event);
        match pipeline.process(raw) {
            Ok(observation) => {
                if let Err(error) = persist_exec(&mut store, &observation, &event) {
                    note(sink, warnings, error.to_string());
                    return Drain::StopStream;
                }
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
    Drain::Continue
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
    if let OpenTcp::Session(session) = open {
        match session.dropped_count() {
            Ok(total) => ingest.record_kernel_drops(total),
            Err(error) => note(sink, warnings, error.to_string()),
        }
    }
    let mut last_ts = twin_core::TimestampNs::now().as_i64();
    for _ in 0..32768 {
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
            collector_timings_ms: Default::default(),
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
        collector_timings_ms: Default::default(),
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
    if !warnings.contains(&message) {
        if warnings.len() == 256 {
            warnings.remove(0);
        }
        warnings.push(message);
    }
}

fn persist_exec(
    store: &mut Store,
    observation: &twin_observation::Observation,
    event: &EbpfExec,
) -> Result<(), AppError> {
    store
        .with_transaction(|store| {
            let mut history = super::scan_history::ScanHistorySession::new(
                observation.timestamp().as_i64(),
                None,
                false,
                false,
                super::scan_history::FinalizeScope::Host,
            );
            let mut runtime_uncertain = Vec::new();
            for owner in
                store.list_edges_to(&twin_core::NodeId::process(event.pid()).to_string())?
            {
                if owner.kind != "owns" {
                    continue;
                }
                for edge in store.list_edges_from(&owner.from_node_id)? {
                    if edge.kind != "connects_to"
                        || edge.last_seen_ns > observation.timestamp().as_i64()
                    {
                        continue;
                    }
                    let metadata: serde_json::Value =
                        serde_json::from_str(&edge.metadata_json).unwrap_or_default();
                    if metadata.get("source").and_then(|value| value.as_str()) != Some("ebpf_tcp") {
                        continue;
                    }
                    let mut has_other_origin = false;
                    for (id, _) in store.list_observations_for_edge(&edge.id)? {
                        if let Some(observation) = store.get_observation(&id)? {
                            if observation.kind == "ebpf_connect"
                                && observation
                                    .subject_node_id
                                    .as_deref()
                                    .is_some_and(|subject| {
                                        subject != format!("process:pid:{}", event.pid())
                                    })
                            {
                                has_other_origin = true;
                                break;
                            }
                        }
                    }
                    if !has_other_origin {
                        runtime_uncertain.push(edge.id);
                    }
                }
            }
            store.insert_observation_typed(observation)?;
            if !super::ebpf_tcp::ensure_process(
                store,
                &mut history,
                event.pid(),
                observation.timestamp(),
                event.start_ticks(),
            )? {
                return Ok(());
            }
            store.mark_edges_missing(
                &runtime_uncertain,
                observation.timestamp().as_i64(),
                true,
                history.collector_run_id(),
            )?;
            let id = twin_core::NodeId::process(event.pid());
            if let Some(existing) = store.get_node_typed(&id)? {
                let mut metadata: serde_json::Value =
                    serde_json::from_str(existing.metadata().as_str())
                        .unwrap_or_else(|_| serde_json::json!({}));
                metadata["last_exec_ns"] = observation.timestamp().as_i64().into();
                metadata["exec_observation_id"] = observation.id().to_string().into();
                if let Some(namespace) = event.network_namespace() {
                    metadata["network_namespace"] = namespace.to_string().into();
                }
                let node = twin_core::GraphNode::from_parts(twin_core::GraphNodeParts {
                    id: id.clone(),
                    kind: NodeKind::Process,
                    label: event.comm().to_string(),
                    state: twin_core::NodeState::Active,
                    first_seen: existing.first_seen(),
                    last_seen: observation.timestamp(),
                    valid_from: existing.valid_from(),
                    valid_to: None,
                    metadata: twin_core::GraphMetadata::from_json(metadata.to_string()),
                });
                super::scan_history::upsert_node(store, &mut history, &node)?;
                let uncertain: Vec<_> = store
                    .list_edges_from(id.as_str())?
                    .into_iter()
                    .filter(|edge| {
                        matches!(
                            edge.kind.as_str(),
                            "connects_to" | "listens_on" | "loads_library" | "uses" | "references"
                        ) && edge.last_seen_ns <= observation.timestamp().as_i64()
                    })
                    .map(|edge| edge.id)
                    .collect();
                store.mark_edges_missing(
                    &uncertain,
                    observation.timestamp().as_i64(),
                    true,
                    history.collector_run_id(),
                )?;
            }
            super::ebpf_cgroup::attribute(
                store,
                &mut history,
                observation,
                event.pid(),
                event.cgroup_id(),
            )?;
            Ok(())
        })
        .map_err(ScanError::Store)?;
    Ok(())
}

fn persist_exec_drops(store: &mut Store, count: u64) -> Result<(), AppError> {
    let mut raw = twin_ebpf::dropped_observation(count, twin_core::TimestampNs::now().as_i64());
    raw.collector = twin_core::CollectorName::new("ebpf_exec");
    raw.metadata.insert_str("source", "ebpf_exec");
    raw.raw_ref = Some(twin_observation::RawEvidenceRef::new("ebpf:exec:dropped"));
    let observation = twin_observation::Pipeline::default()
        .process(raw)
        .map_err(ScanError::Observation)?;
    store
        .with_transaction(|store| {
            for row in store.list_observations_by_kind("ebpf_dropped_events")? {
                let metadata: serde_json::Value =
                    serde_json::from_str(&row.metadata_json).unwrap_or_default();
                if metadata.get("source").and_then(|value| value.as_str()) == Some("ebpf_exec") {
                    store.delete_observation(&row.id)?;
                }
            }
            store.insert_observation_typed(&observation)?;
            Ok(())
        })
        .map_err(ScanError::Store)?;
    Ok(())
}
