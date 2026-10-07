use std::collections::HashMap;

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawObservation,
};

pub const DEFAULT_PER_PID: u32 = 64;
pub const DEFAULT_PER_ENDPOINT: u32 = 128;
pub const DEFAULT_GLOBAL: u32 = 512;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpRateLimits {
    pub per_pid: u32,
    pub per_endpoint: u32,
    pub global_limit: u32,
}

impl TcpRateLimits {
    pub fn session() -> Self {
        Self {
            per_pid: DEFAULT_PER_PID,
            per_endpoint: DEFAULT_PER_ENDPOINT,
            global_limit: DEFAULT_GLOBAL,
        }
    }
}

#[derive(Debug, Clone)]
pub struct TcpRateLimiter {
    limits: TcpRateLimits,
    pid_counts: HashMap<u32, u32>,
    endpoint_counts: HashMap<String, u32>,
    global_count: u32,
    dropped: u64,
}

impl TcpRateLimiter {
    pub fn new(limits: TcpRateLimits) -> Self {
        Self {
            limits,
            pid_counts: HashMap::new(),
            endpoint_counts: HashMap::new(),
            global_count: 0,
            dropped: 0,
        }
    }

    pub fn admit(&mut self, pid: u32, endpoint: &str) -> bool {
        let pid_count = self.pid_counts.get(&pid).copied().unwrap_or(0);
        let endpoint_count = self.endpoint_counts.get(endpoint).copied().unwrap_or(0);
        if self.global_count >= self.limits.global_limit
            || pid_count >= self.limits.per_pid
            || endpoint_count >= self.limits.per_endpoint
        {
            self.dropped = self.dropped.saturating_add(1);
            return false;
        }
        self.global_count = self.global_count.saturating_add(1);
        *self.pid_counts.entry(pid).or_insert(0) = pid_count.saturating_add(1);
        self.endpoint_counts
            .insert(endpoint.to_string(), endpoint_count.saturating_add(1));
        true
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }
}

pub fn dropped_observation(count: u64, timestamp_ns: i64) -> RawObservation {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("source", "ebpf_tcp");
    metadata.insert_u64("count", count);
    RawObservation {
        source: ObservationSource::Ebpf,
        kind: ObservationKind::EbpfDroppedEvents,
        collector: CollectorName::new("ebpf_tcp"),
        subject: None,
        object: None,
        timestamp: TimestampNs::new(timestamp_ns),
        raw_ref: Some(RawEvidenceRef::new("ebpf:tcp:dropped")),
        confidence_hint: ConfidenceHint::Low,
        metadata,
    }
}
