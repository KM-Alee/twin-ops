use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::error::EbpfError;

pub const EXEC_EVENT_LEN: usize = 40;
const COMM_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EbpfExec {
    pid: u32,
    ppid: u32,
    comm: String,
    cgroup_id: u64,
    timestamp_ns: u64,
}

impl EbpfExec {
    pub fn new(
        pid: u32,
        ppid: u32,
        comm: impl Into<String>,
        cgroup_id: u64,
        timestamp_ns: u64,
    ) -> Self {
        Self {
            pid,
            ppid,
            comm: bound_comm(comm.into().as_str()),
            cgroup_id,
            timestamp_ns,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn ppid(&self) -> u32 {
        self.ppid
    }

    pub fn comm(&self) -> &str {
        &self.comm
    }

    pub fn cgroup_id(&self) -> u64 {
        self.cgroup_id
    }

    pub fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }
}

pub fn decode_exec(bytes: &[u8]) -> Result<EbpfExec, EbpfError> {
    if bytes.len() != EXEC_EVENT_LEN {
        return Err(EbpfError::Malformed {
            reason: format!(
                "exec event is {} bytes, expected {EXEC_EVENT_LEN}",
                bytes.len()
            ),
        });
    }
    let pid = read_u32(bytes, 0);
    let ppid = read_u32(bytes, 4);
    let cgroup_id = read_u64(bytes, 8);
    let timestamp_ns = read_u64(bytes, 16);
    let comm = comm_from_bytes(&bytes[24..40]);
    Ok(EbpfExec {
        pid,
        ppid,
        comm,
        cgroup_id,
        timestamp_ns,
    })
}

pub fn encode_exec(event: &EbpfExec) -> [u8; EXEC_EVENT_LEN] {
    let mut out = [0u8; EXEC_EVENT_LEN];
    out[0..4].copy_from_slice(&event.pid.to_le_bytes());
    out[4..8].copy_from_slice(&event.ppid.to_le_bytes());
    out[8..16].copy_from_slice(&event.cgroup_id.to_le_bytes());
    out[16..24].copy_from_slice(&event.timestamp_ns.to_le_bytes());
    let comm = event.comm.as_bytes();
    let n = comm.len().min(COMM_LEN);
    out[24..24 + n].copy_from_slice(&comm[..n]);
    out
}

pub fn exec_to_raw(event: &EbpfExec) -> RawObservation {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_u32("ppid", event.ppid);
    metadata.insert_str("comm", &event.comm);
    metadata.insert_str("cgroup_id", &event.cgroup_id.to_string());
    let timestamp = i64::try_from(event.timestamp_ns).unwrap_or(i64::MAX);
    RawObservation {
        source: ObservationSource::Ebpf,
        kind: ObservationKind::EbpfExecObserved,
        collector: CollectorName::new("ebpf_exec"),
        subject: Some(RawIdentity::Process { pid: event.pid }),
        object: None,
        timestamp: TimestampNs::new(timestamp),
        raw_ref: Some(RawEvidenceRef::new(format!("ebpf:exec:{}", event.pid))),
        confidence_hint: ConfidenceHint::High,
        metadata,
    }
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&bytes[offset..offset + 4]);
    u32::from_le_bytes(buf)
}

fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&bytes[offset..offset + 8]);
    u64::from_le_bytes(buf)
}

fn comm_from_bytes(raw: &[u8]) -> String {
    let end = raw.iter().position(|byte| *byte == 0).unwrap_or(raw.len());
    let text = String::from_utf8_lossy(&raw[..end]);
    text.chars()
        .filter(|ch| *ch != '\n' && *ch != '\r')
        .collect()
}

fn bound_comm(comm: &str) -> String {
    let mut bytes = Vec::new();
    for byte in comm.bytes() {
        if byte == 0 || bytes.len() == COMM_LEN {
            break;
        }
        if byte == b'\n' || byte == b'\r' {
            continue;
        }
        bytes.push(byte);
    }
    String::from_utf8_lossy(&bytes).into_owned()
}
