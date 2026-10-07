use serde::Serialize;
use twin_app::{CollectorTiming, WatchResult, WatchStop, WatchTick};

use crate::output::format::{format_duration_ns, Lines, Status};

pub fn render_header(interval: &str, duration: Option<&str>, ticks: Option<u32>) -> String {
    let mut out = Lines::new();
    out.title("twin watch");
    out.status_row(Status::Ok, "watching", &format!("every {interval}"));
    out.status_row(Status::Neutral, "stop", &stop_plan(duration, ticks));
    out.blank();
    out.into_string()
}

pub fn render_tick(tick: &WatchTick) -> String {
    let clock = format_local_hms(tick.at_ns);
    let scan = format_duration_ns(tick.scan_started_at_ns, tick.scan_ended_at_ns);
    format!("  [{clock}] {}  · scan {scan}", change_summary(tick))
}

pub fn render_summary(result: &WatchResult) -> String {
    let mut out = Lines::new();
    out.blank();
    out.status_row(
        Status::Ok,
        "stopped",
        &format!(
            "{} · {} · {}",
            stop_label(result.stop),
            count_phrase(result.ticks, "scan", "scans"),
            count_phrase(result.event_count, "event", "events")
        ),
    );
    out.status_row(
        Status::Neutral,
        "elapsed",
        &format_duration_ns(result.started_at_ns, result.ended_at_ns),
    );
    if !result.collector_timings.is_empty() {
        out.status_row(
            Status::Neutral,
            "collectors",
            &timing_phrase(&result.collector_timings),
        );
    }
    out.into_string()
}

pub fn render_tick_json(tick: &WatchTick) -> String {
    render_json(&JsonTick {
        record: "tick",
        body: tick,
    })
}

pub fn render_summary_json(result: &WatchResult) -> String {
    render_json(&JsonSummary {
        record: "summary",
        body: result,
    })
}

pub fn change_summary(tick: &WatchTick) -> String {
    let mut parts = Vec::new();
    push_signed(
        &mut parts,
        '+',
        tick.processes_added,
        "process",
        "processes",
    );
    push_signed(
        &mut parts,
        '-',
        tick.processes_removed,
        "process",
        "processes",
    );
    push_signed(
        &mut parts,
        '+',
        tick.connections_added,
        "connection",
        "connections",
    );
    push_signed(
        &mut parts,
        '-',
        tick.connections_removed,
        "connection",
        "connections",
    );
    push_signed(
        &mut parts,
        '+',
        tick.listening_ports_added,
        "listening port",
        "listening ports",
    );
    push_signed(
        &mut parts,
        '-',
        tick.listening_ports_removed,
        "listening port",
        "listening ports",
    );
    push_signed(&mut parts, '+', tick.services_added, "service", "services");
    push_signed(
        &mut parts,
        '-',
        tick.services_removed,
        "service",
        "services",
    );
    push_stale(&mut parts, tick.processes_stale, "process", "processes");
    push_stale(
        &mut parts,
        tick.listening_ports_stale,
        "listening port",
        "listening ports",
    );
    push_stale(&mut parts, tick.services_stale, "service", "services");
    if parts.is_empty() {
        "no significant changes".to_string()
    } else {
        parts.join(", ")
    }
}

fn stop_plan(duration: Option<&str>, ticks: Option<u32>) -> String {
    match (duration, ticks) {
        (Some(duration), Some(ticks)) => format!("after {duration} or {ticks} scans"),
        (Some(duration), None) => format!("after {duration}"),
        (None, Some(1)) => "after 1 scan".to_string(),
        (None, Some(ticks)) => format!("after {ticks} scans"),
        (None, None) => "until interrupted".to_string(),
    }
}

fn stop_label(stop: WatchStop) -> &'static str {
    match stop {
        WatchStop::Interrupted => "interrupted",
        WatchStop::Duration => "duration elapsed",
        WatchStop::MaxTicks => "tick limit",
    }
}

fn count_phrase(count: impl Into<u64>, singular: &str, plural: &str) -> String {
    let count = count.into();
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn push_signed(parts: &mut Vec<String>, sign: char, count: u64, singular: &str, plural: &str) {
    if count == 0 {
        return;
    }
    let noun = if count == 1 { singular } else { plural };
    parts.push(format!("{sign}{count} {noun}"));
}

fn push_stale(parts: &mut Vec<String>, count: u64, singular: &str, plural: &str) {
    if count == 0 {
        return;
    }
    let noun = if count == 1 { singular } else { plural };
    parts.push(format!("{count} stale {noun}"));
}

fn timing_phrase(timings: &[CollectorTiming]) -> String {
    timings
        .iter()
        .map(|timing| {
            let duration = format_duration_ns(0, timing.total_duration_ns);
            if timing.runs > 1 {
                format!("{} {duration} ({} runs)", timing.collector, timing.runs)
            } else {
                format!("{} {duration}", timing.collector)
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

fn render_json<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|error| format!("{{\"error\":\"{error}\"}}"))
}

#[derive(Serialize)]
struct JsonTick<'a> {
    record: &'static str,
    #[serde(flatten)]
    body: &'a WatchTick,
}

#[derive(Serialize)]
struct JsonSummary<'a> {
    record: &'static str,
    #[serde(flatten)]
    body: &'a WatchResult,
}

fn format_local_hms(unix_ns: i64) -> String {
    let secs = unix_ns.div_euclid(1_000_000_000);
    match local_hms(secs) {
        Some((hour, minute, second)) => format!("{hour:02}:{minute:02}:{second:02}"),
        None => {
            let day = secs.rem_euclid(86_400) as u64;
            format!("{:02}:{:02}:{:02}", day / 3600, (day % 3600) / 60, day % 60)
        }
    }
}

fn local_hms(unix_secs: i64) -> Option<(i32, i32, i32)> {
    let secs = unix_secs as libc::time_t;
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // std has no local-time formatter; localtime_r is the thread-safe libc conversion.
    let ptr = unsafe { libc::localtime_r(&secs, tm.as_mut_ptr()) };
    if ptr.is_null() {
        return None;
    }
    let tm = unsafe { tm.assume_init() };
    Some((tm.tm_hour, tm.tm_min, tm.tm_sec))
}
