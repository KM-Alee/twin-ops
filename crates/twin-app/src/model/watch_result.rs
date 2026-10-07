use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct WatchResult {
    pub interval_secs: u64,
    pub duration_secs: Option<u64>,
    pub ticks_completed: u32,
    pub exec_count: u64,
    pub tcp_count: u64,
    pub ebpf_requested: bool,
    pub ebpf_attached: bool,
    pub warnings: Vec<String>,
    pub ticks: Vec<WatchTick>,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchTick {
    pub at_ns: i64,
    pub baseline: bool,
    pub processes: u64,
    pub connections: u64,
    pub listening_ports: u64,
    pub processes_added: u64,
    pub processes_removed: u64,
    pub connections_added: u64,
    pub connections_removed: u64,
    pub listening_ports_added: u64,
    pub listening_ports_removed: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchExecEvent {
    pub node_id: String,
    pub comm: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct WatchTcpEvent {
    pub node_id: String,
    pub action: String,
    pub endpoint: String,
}
