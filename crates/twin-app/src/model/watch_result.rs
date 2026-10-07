use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchStop {
    Interrupted,
    Duration,
    MaxTicks,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CollectorTiming {
    pub collector: String,
    pub total_duration_ns: i64,
    pub runs: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchTick {
    pub index: u32,
    pub at_ns: i64,
    pub scan_started_at_ns: i64,
    pub scan_ended_at_ns: i64,
    pub processes_added: u64,
    pub processes_removed: u64,
    pub connections_added: u64,
    pub connections_removed: u64,
    pub listening_ports_added: u64,
    pub listening_ports_removed: u64,
    pub services_added: u64,
    pub services_removed: u64,
    pub processes_stale: u64,
    pub listening_ports_stale: u64,
    pub services_stale: u64,
    pub event_count: u64,
    pub collector_timings: Vec<CollectorTiming>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct WatchResult {
    pub interval_secs: u64,
    pub duration_secs: Option<u64>,
    pub ticks: u32,
    pub event_count: u64,
    pub stop: WatchStop,
    pub started_at_ns: i64,
    pub ended_at_ns: i64,
    pub collector_timings: Vec<CollectorTiming>,
}
