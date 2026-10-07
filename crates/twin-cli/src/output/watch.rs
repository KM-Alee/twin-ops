use twin_app::{WatchExecEvent, WatchTick};

use crate::output::format::{Lines, Status};

pub fn render_banner(interval_secs: u64) -> String {
    let mut out = Lines::new();
    out.title("twin watch");
    out.status_row(Status::Ok, "watching", &format!("every {interval_secs}s"));
    out.into_string()
}

pub fn render_tick(tick: &WatchTick) -> String {
    let stamp = format_hms(tick.at_ns);
    if tick.baseline {
        return format!(
            "[{stamp}] baseline: {}, {}, {}",
            count_phrase(tick.processes, "process", "processes"),
            count_phrase(tick.connections, "connection", "connections"),
            count_phrase(tick.listening_ports, "listening port", "listening ports"),
        );
    }
    let mut parts = Vec::new();
    push_signed(
        &mut parts,
        tick.processes_added,
        true,
        "process",
        "processes",
    );
    push_signed(
        &mut parts,
        tick.processes_removed,
        false,
        "process",
        "processes",
    );
    push_signed(
        &mut parts,
        tick.connections_added,
        true,
        "connection",
        "connections",
    );
    push_signed(
        &mut parts,
        tick.connections_removed,
        false,
        "connection",
        "connections",
    );
    push_signed(
        &mut parts,
        tick.listening_ports_added,
        true,
        "listening port",
        "listening ports",
    );
    push_signed(
        &mut parts,
        tick.listening_ports_removed,
        false,
        "listening port",
        "listening ports",
    );
    if parts.is_empty() {
        format!("[{stamp}] no significant changes")
    } else {
        format!("[{stamp}] {}", parts.join(", "))
    }
}

pub fn render_exec(event: &WatchExecEvent) -> String {
    if event.comm.is_empty() {
        format!("[exec] {}", event.node_id)
    } else {
        format!("[exec] {} {}", event.node_id, event.comm)
    }
}

pub fn render_warning(message: &str) -> String {
    let mut out = Lines::new();
    out.status_row(Status::Warn, "eBPF", message);
    out.into_string()
}

fn push_signed(parts: &mut Vec<String>, count: u64, positive: bool, singular: &str, plural: &str) {
    if count == 0 {
        return;
    }
    let sign = if positive { "+" } else { "-" };
    parts.push(format!("{sign}{}", count_phrase(count, singular, plural)));
}

fn count_phrase(count: u64, singular: &str, plural: &str) -> String {
    if count == 1 {
        format!("1 {singular}")
    } else {
        format!("{count} {plural}")
    }
}

fn format_hms(ns: i64) -> String {
    let secs = ns.div_euclid(1_000_000_000).rem_euclid(86_400) as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
