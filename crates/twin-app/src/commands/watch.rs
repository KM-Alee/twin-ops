use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use twin_core::TimestampNs;
use twin_store::{CollectorRunRow, EdgeHistoryRow, NodeHistoryRow, Store};

use crate::commands::duration::parse_duration_secs;
use crate::error::{TemporalError, WatchError};
use crate::model::{CollectorTiming, WatchResult, WatchStop, WatchTick};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::{AppError, ScanRequest, WatchRequest};

const STOP_POLL: Duration = Duration::from_millis(100);

struct Bounds {
    interval: Duration,
    interval_secs: u64,
    duration: Option<Duration>,
    duration_secs: Option<u64>,
    max_ticks: Option<u32>,
}

enum Pause {
    Elapsed,
    Interrupted,
    Deadline,
}

pub fn run_home(
    request: WatchRequest,
    proc_root: &Path,
    stop: &AtomicBool,
    on_tick: impl FnMut(&WatchTick),
) -> Result<WatchResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request, proc_root, stop, on_tick)
}

pub fn run(
    layout: &TwinLayout,
    request: &WatchRequest,
    proc_root: &Path,
    stop: &AtomicBool,
    mut on_tick: impl FnMut(&WatchTick),
) -> Result<WatchResult, AppError> {
    let bounds = bounds(request)?;
    let db_path = layout.db_file();
    ensure_ready(&db_path)?;

    let started = Instant::now();
    let started_at_ns = TimestampNs::now().as_i64();
    let deadline = bounds.duration.map(|duration| started + duration);
    let scan_request = ScanRequest {
        config_override: request.config_override.clone(),
        samples: 1,
        interval_secs: 1,
    };

    let mut ticks = 0u32;
    let mut event_count = 0u64;
    let mut timings: BTreeMap<String, CollectorTiming> = BTreeMap::new();

    let stop_reason = loop {
        if stop.load(Ordering::SeqCst) {
            break WatchStop::Interrupted;
        }
        if deadline_passed(deadline) {
            break WatchStop::Duration;
        }
        if bounds.max_ticks.is_some_and(|max_ticks| ticks >= max_ticks) {
            break WatchStop::MaxTicks;
        }

        let since_ns = TimestampNs::now().as_i64();
        let scan = crate::scan_in(layout, scan_request.clone(), proc_root)?;
        let (delta, collector_timings) = read_delta(&db_path, since_ns)?;
        ticks = ticks.saturating_add(1);
        let tick = WatchTick {
            index: ticks,
            at_ns: scan.ended_at_ns,
            scan_started_at_ns: scan.started_at_ns,
            scan_ended_at_ns: scan.ended_at_ns,
            processes_added: delta.processes_added,
            processes_removed: delta.processes_removed,
            connections_added: delta.connections_added,
            connections_removed: delta.connections_removed,
            listening_ports_added: delta.listening_ports_added,
            listening_ports_removed: delta.listening_ports_removed,
            services_added: delta.services_added,
            services_removed: delta.services_removed,
            processes_stale: delta.processes_stale,
            listening_ports_stale: delta.listening_ports_stale,
            services_stale: delta.services_stale,
            event_count: delta.event_count(),
            collector_timings,
        };
        event_count = event_count.saturating_add(tick.event_count);
        merge_timings(&mut timings, &tick.collector_timings);
        on_tick(&tick);

        if stop.load(Ordering::SeqCst) {
            break WatchStop::Interrupted;
        }
        if bounds.max_ticks.is_some_and(|max_ticks| ticks >= max_ticks) {
            break WatchStop::MaxTicks;
        }
        if deadline_passed(deadline) {
            break WatchStop::Duration;
        }
        match pause(bounds.interval, deadline, stop) {
            Pause::Elapsed => {}
            Pause::Interrupted => break WatchStop::Interrupted,
            Pause::Deadline => break WatchStop::Duration,
        }
    };

    Ok(WatchResult {
        interval_secs: bounds.interval_secs,
        duration_secs: bounds.duration_secs,
        ticks,
        event_count,
        stop: stop_reason,
        started_at_ns,
        ended_at_ns: TimestampNs::now().as_i64(),
        collector_timings: timings.into_values().collect(),
    })
}

fn bounds(request: &WatchRequest) -> Result<Bounds, AppError> {
    let interval_secs = if request.interval.trim() == "0s" {
        0
    } else {
        parse_duration_secs(&request.interval)
            .map_err(|error| map_interval(error, &request.interval))?
    };
    let duration_secs = match &request.duration {
        Some(duration) => {
            Some(parse_duration_secs(duration).map_err(|error| map_duration(error, duration))?)
        }
        None => None,
    };
    if let Some(0) = request.max_ticks {
        return Err(WatchError::InvalidTickCount {
            value: 0,
            reason: "tick count must be at least 1".to_string(),
        }
        .into());
    }
    Ok(Bounds {
        interval: Duration::from_secs(interval_secs),
        interval_secs,
        duration: duration_secs.map(Duration::from_secs),
        duration_secs,
        max_ticks: request.max_ticks,
    })
}

fn map_interval(error: TemporalError, value: &str) -> WatchError {
    match error {
        TemporalError::InvalidDuration { value, reason } => {
            WatchError::InvalidInterval { value, reason }
        }
        other => WatchError::InvalidInterval {
            value: value.to_string(),
            reason: other.to_string(),
        },
    }
}

fn map_duration(error: TemporalError, value: &str) -> WatchError {
    match error {
        TemporalError::InvalidDuration { value, reason } => {
            WatchError::InvalidDuration { value, reason }
        }
        other => WatchError::InvalidDuration {
            value: value.to_string(),
            reason: other.to_string(),
        },
    }
}

fn ensure_ready(db_path: &Path) -> Result<(), AppError> {
    if !db_path.exists() {
        return Err(WatchError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(WatchError::StoreOpen)?;
    if !store.is_initialized().map_err(WatchError::Store)? {
        return Err(WatchError::DatabaseNotInitialized.into());
    }
    Ok(())
}

struct Delta {
    processes_added: u64,
    processes_removed: u64,
    connections_added: u64,
    connections_removed: u64,
    listening_ports_added: u64,
    listening_ports_removed: u64,
    services_added: u64,
    services_removed: u64,
    processes_stale: u64,
    listening_ports_stale: u64,
    services_stale: u64,
}

impl Delta {
    fn event_count(&self) -> u64 {
        self.processes_added
            + self.processes_removed
            + self.connections_added
            + self.connections_removed
            + self.listening_ports_added
            + self.listening_ports_removed
            + self.services_added
            + self.services_removed
            + self.processes_stale
            + self.listening_ports_stale
            + self.services_stale
    }
}

fn read_delta(db_path: &Path, since_ns: i64) -> Result<(Delta, Vec<CollectorTiming>), AppError> {
    let store = Store::open(db_path).map_err(WatchError::StoreOpen)?;
    let nodes = store
        .list_node_history_since(since_ns)
        .map_err(WatchError::Store)?;
    let edges = store
        .list_edge_history_since(since_ns)
        .map_err(WatchError::Store)?;
    let runs = store
        .list_collector_runs_since(since_ns)
        .map_err(WatchError::Store)?;
    Ok((summarize(&nodes, &edges), timings_from_runs(&runs)))
}

// Operational changes only. File, cgroup, and parent-edge churn stays in history
// without a watch line so the stream stays readable.
fn summarize(nodes: &[NodeHistoryRow], edges: &[EdgeHistoryRow]) -> Delta {
    let mut delta = Delta {
        processes_added: 0,
        processes_removed: 0,
        connections_added: 0,
        connections_removed: 0,
        listening_ports_added: 0,
        listening_ports_removed: 0,
        services_added: 0,
        services_removed: 0,
        processes_stale: 0,
        listening_ports_stale: 0,
        services_stale: 0,
    };
    for node in nodes {
        note_node(&mut delta, &node.kind, &node.change_kind);
    }
    for edge in edges {
        note_connection(&mut delta, edge);
    }
    delta
}

fn note_node(delta: &mut Delta, kind: &str, change: &str) {
    let Some(slot) = (match (kind, change) {
        ("process", "new" | "reappeared") => Some(&mut delta.processes_added),
        ("process", "gone") => Some(&mut delta.processes_removed),
        ("process", "stale") => Some(&mut delta.processes_stale),
        ("port", "new" | "reappeared") => Some(&mut delta.listening_ports_added),
        ("port", "gone") => Some(&mut delta.listening_ports_removed),
        ("port", "stale") => Some(&mut delta.listening_ports_stale),
        ("service", "new" | "reappeared") => Some(&mut delta.services_added),
        ("service", "gone") => Some(&mut delta.services_removed),
        ("service", "stale") => Some(&mut delta.services_stale),
        _ => None,
    }) else {
        return;
    };
    *slot = slot.saturating_add(1);
}

fn note_connection(delta: &mut Delta, edge: &EdgeHistoryRow) {
    if edge.kind != "connects_to" || !edge.from_node_id.starts_with("process:") {
        return;
    }
    match edge.change_kind.as_str() {
        "new" | "reappeared" => {
            delta.connections_added = delta.connections_added.saturating_add(1);
        }
        "gone" => {
            delta.connections_removed = delta.connections_removed.saturating_add(1);
        }
        _ => {}
    }
}

fn timings_from_runs(runs: &[CollectorRunRow]) -> Vec<CollectorTiming> {
    let mut totals: BTreeMap<String, CollectorTiming> = BTreeMap::new();
    for run in runs {
        let duration = run.ended_at_ns.saturating_sub(run.started_at_ns);
        let entry = totals
            .entry(run.collector.clone())
            .or_insert_with(|| CollectorTiming {
                collector: run.collector.clone(),
                total_duration_ns: 0,
                runs: 0,
            });
        entry.total_duration_ns = entry.total_duration_ns.saturating_add(duration);
        entry.runs = entry.runs.saturating_add(1);
    }
    totals.into_values().collect()
}

fn merge_timings(total: &mut BTreeMap<String, CollectorTiming>, tick: &[CollectorTiming]) {
    for timing in tick {
        let entry = total
            .entry(timing.collector.clone())
            .or_insert_with(|| CollectorTiming {
                collector: timing.collector.clone(),
                total_duration_ns: 0,
                runs: 0,
            });
        entry.total_duration_ns = entry
            .total_duration_ns
            .saturating_add(timing.total_duration_ns);
        entry.runs = entry.runs.saturating_add(timing.runs);
    }
}

fn deadline_passed(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|deadline| Instant::now() >= deadline)
}

fn pause(interval: Duration, deadline: Option<Instant>, stop: &AtomicBool) -> Pause {
    if stop.load(Ordering::SeqCst) {
        return Pause::Interrupted;
    }
    if interval.is_zero() {
        return if deadline_passed(deadline) {
            Pause::Deadline
        } else {
            Pause::Elapsed
        };
    }
    let sleep_end = Instant::now() + interval;
    loop {
        if stop.load(Ordering::SeqCst) {
            return Pause::Interrupted;
        }
        let now = Instant::now();
        if deadline_passed(deadline) {
            return Pause::Deadline;
        }
        if now >= sleep_end {
            return Pause::Elapsed;
        }
        let mut step = STOP_POLL.min(sleep_end.saturating_duration_since(now));
        if let Some(deadline) = deadline {
            let until_deadline = deadline.saturating_duration_since(now);
            if until_deadline < step {
                step = until_deadline;
            }
        }
        if step.is_zero() {
            return if stop.load(Ordering::SeqCst) {
                Pause::Interrupted
            } else {
                Pause::Deadline
            };
        }
        thread::sleep(step);
    }
}
