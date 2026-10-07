use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::error::EbpfError;

pub const TCP_CONNECT_LEN: usize = 56;
pub const TCP_ACCEPT_LEN: usize = 58;
pub const TCP_BIND_LEN: usize = 40;
const ADDR_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConnect {
    pid: u32,
    cgroup_id: u64,
    saddr: IpAddr,
    daddr: IpAddr,
    dport: u16,
    timestamp_ns: u64,
}

impl TcpConnect {
    pub fn new(
        pid: u32,
        cgroup_id: u64,
        saddr: IpAddr,
        daddr: IpAddr,
        dport: u16,
        timestamp_ns: u64,
    ) -> Self {
        Self {
            pid,
            cgroup_id,
            saddr,
            daddr,
            dport,
            timestamp_ns,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn cgroup_id(&self) -> u64 {
        self.cgroup_id
    }

    pub fn saddr(&self) -> IpAddr {
        self.saddr
    }

    pub fn daddr(&self) -> IpAddr {
        self.daddr
    }

    pub fn dport(&self) -> u16 {
        self.dport
    }

    pub fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    pub fn endpoint(&self) -> String {
        format_endpoint(self.daddr, self.dport)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpAccept {
    pid: u32,
    cgroup_id: u64,
    saddr: IpAddr,
    sport: u16,
    daddr: IpAddr,
    dport: u16,
    timestamp_ns: u64,
}

impl TcpAccept {
    pub fn new(
        pid: u32,
        cgroup_id: u64,
        saddr: IpAddr,
        sport: u16,
        daddr: IpAddr,
        dport: u16,
        timestamp_ns: u64,
    ) -> Self {
        Self {
            pid,
            cgroup_id,
            saddr,
            sport,
            daddr,
            dport,
            timestamp_ns,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn cgroup_id(&self) -> u64 {
        self.cgroup_id
    }

    pub fn saddr(&self) -> IpAddr {
        self.saddr
    }

    pub fn sport(&self) -> u16 {
        self.sport
    }

    pub fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    pub fn endpoint(&self) -> String {
        format_endpoint(self.saddr, self.sport)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpBind {
    pid: u32,
    cgroup_id: u64,
    addr: IpAddr,
    port: u16,
    timestamp_ns: u64,
}

impl TcpBind {
    pub fn new(pid: u32, cgroup_id: u64, addr: IpAddr, port: u16, timestamp_ns: u64) -> Self {
        Self {
            pid,
            cgroup_id,
            addr,
            port,
            timestamp_ns,
        }
    }

    pub fn pid(&self) -> u32 {
        self.pid
    }

    pub fn cgroup_id(&self) -> u64 {
        self.cgroup_id
    }

    pub fn addr(&self) -> IpAddr {
        self.addr
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn timestamp_ns(&self) -> u64 {
        self.timestamp_ns
    }

    pub fn endpoint(&self) -> String {
        format_endpoint(self.addr, self.port)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpEvent {
    Connect(TcpConnect),
    Accept(TcpAccept),
    Bind(TcpBind),
}

impl TcpEvent {
    pub fn pid(&self) -> u32 {
        match self {
            Self::Connect(event) => event.pid(),
            Self::Accept(event) => event.pid(),
            Self::Bind(event) => event.pid(),
        }
    }

    pub fn endpoint(&self) -> String {
        match self {
            Self::Connect(event) => event.endpoint(),
            Self::Accept(event) => event.endpoint(),
            Self::Bind(event) => event.endpoint(),
        }
    }

    pub fn action(&self) -> &'static str {
        match self {
            Self::Connect(_) => "connect",
            Self::Accept(_) => "accept",
            Self::Bind(_) => "bind",
        }
    }

    pub fn timestamp_ns(&self) -> u64 {
        match self {
            Self::Connect(event) => event.timestamp_ns(),
            Self::Accept(event) => event.timestamp_ns(),
            Self::Bind(event) => event.timestamp_ns(),
        }
    }
}

pub fn decode_tcp_event(bytes: &[u8]) -> Result<TcpEvent, EbpfError> {
    match bytes.len() {
        TCP_CONNECT_LEN => decode_connect(bytes).map(TcpEvent::Connect),
        TCP_ACCEPT_LEN => decode_accept(bytes).map(TcpEvent::Accept),
        TCP_BIND_LEN => decode_bind(bytes).map(TcpEvent::Bind),
        other => Err(malformed(format!(
            "tcp event is {other} bytes, expected {TCP_CONNECT_LEN}, {TCP_ACCEPT_LEN}, or {TCP_BIND_LEN}"
        ))),
    }
}

pub fn decode_connect(bytes: &[u8]) -> Result<TcpConnect, EbpfError> {
    if bytes.len() != TCP_CONNECT_LEN {
        return Err(malformed(format!(
            "connect event is {} bytes, expected {TCP_CONNECT_LEN}",
            bytes.len()
        )));
    }
    let family = bytes[22];
    Ok(TcpConnect {
        pid: read_u32(bytes, 0),
        cgroup_id: read_u64(bytes, 4),
        timestamp_ns: read_u64(bytes, 12),
        dport: read_u16(bytes, 20),
        saddr: read_addr(&bytes[24..40], family)?,
        daddr: read_addr(&bytes[40..56], family)?,
    })
}

pub fn decode_accept(bytes: &[u8]) -> Result<TcpAccept, EbpfError> {
    if bytes.len() != TCP_ACCEPT_LEN {
        return Err(malformed(format!(
            "accept event is {} bytes, expected {TCP_ACCEPT_LEN}",
            bytes.len()
        )));
    }
    let family = bytes[24];
    Ok(TcpAccept {
        pid: read_u32(bytes, 0),
        cgroup_id: read_u64(bytes, 4),
        timestamp_ns: read_u64(bytes, 12),
        sport: read_u16(bytes, 20),
        dport: read_u16(bytes, 22),
        saddr: read_addr(&bytes[26..42], family)?,
        daddr: read_addr(&bytes[42..58], family)?,
    })
}

pub fn decode_bind(bytes: &[u8]) -> Result<TcpBind, EbpfError> {
    if bytes.len() != TCP_BIND_LEN {
        return Err(malformed(format!(
            "bind event is {} bytes, expected {TCP_BIND_LEN}",
            bytes.len()
        )));
    }
    let family = bytes[22];
    Ok(TcpBind {
        pid: read_u32(bytes, 0),
        cgroup_id: read_u64(bytes, 4),
        timestamp_ns: read_u64(bytes, 12),
        port: read_u16(bytes, 20),
        addr: read_addr(&bytes[24..40], family)?,
    })
}

pub fn encode_connect(event: &TcpConnect) -> [u8; TCP_CONNECT_LEN] {
    let mut out = [0u8; TCP_CONNECT_LEN];
    write_u32(&mut out, 0, event.pid);
    write_u64(&mut out, 4, event.cgroup_id);
    write_u64(&mut out, 12, event.timestamp_ns);
    write_u16(&mut out, 20, event.dport);
    let family = write_addr(&mut out[24..40], event.saddr);
    out[22] = family;
    write_addr(&mut out[40..56], event.daddr);
    out
}

pub fn encode_accept(event: &TcpAccept) -> [u8; TCP_ACCEPT_LEN] {
    let mut out = [0u8; TCP_ACCEPT_LEN];
    write_u32(&mut out, 0, event.pid);
    write_u64(&mut out, 4, event.cgroup_id);
    write_u64(&mut out, 12, event.timestamp_ns);
    write_u16(&mut out, 20, event.sport);
    write_u16(&mut out, 22, event.dport);
    let family = write_addr(&mut out[26..42], event.saddr);
    out[24] = family;
    write_addr(&mut out[42..58], event.daddr);
    out
}

pub fn encode_bind(event: &TcpBind) -> [u8; TCP_BIND_LEN] {
    let mut out = [0u8; TCP_BIND_LEN];
    write_u32(&mut out, 0, event.pid);
    write_u64(&mut out, 4, event.cgroup_id);
    write_u64(&mut out, 12, event.timestamp_ns);
    write_u16(&mut out, 20, event.port);
    let family = write_addr(&mut out[24..40], event.addr);
    out[22] = family;
    out
}

pub fn tcp_to_raw(event: &TcpEvent) -> RawObservation {
    match event {
        TcpEvent::Connect(event) => connect_raw(event),
        TcpEvent::Accept(event) => accept_raw(event),
        TcpEvent::Bind(event) => bind_raw(event),
    }
}

fn connect_raw(event: &TcpConnect) -> RawObservation {
    let endpoint = event.endpoint();
    observation(
        ObservationKind::EbpfConnect,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.daddr().to_string(),
            port: event.dport(),
        }),
        format!("ebpf:tcp:connect:{}:{endpoint}", event.pid()),
    )
}

fn accept_raw(event: &TcpAccept) -> RawObservation {
    let endpoint = event.endpoint();
    observation(
        ObservationKind::EbpfAccept,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.saddr().to_string(),
            port: event.sport(),
        }),
        format!("ebpf:tcp:accept:{}:{endpoint}", event.pid()),
    )
}

fn bind_raw(event: &TcpBind) -> RawObservation {
    let endpoint = event.endpoint();
    observation(
        ObservationKind::EbpfBind,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.addr().to_string(),
            port: event.port(),
        }),
        format!("ebpf:tcp:bind:{}:{endpoint}", event.pid()),
    )
}

fn observation(
    kind: ObservationKind,
    pid: u32,
    cgroup_id: u64,
    timestamp_ns: u64,
    object: Option<RawIdentity>,
    raw_ref: String,
) -> RawObservation {
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("cgroup_id", &cgroup_id.to_string());
    let timestamp = i64::try_from(timestamp_ns).unwrap_or(i64::MAX);
    RawObservation {
        source: ObservationSource::Ebpf,
        kind,
        collector: CollectorName::new("ebpf_tcp"),
        subject: Some(RawIdentity::Process { pid }),
        object,
        timestamp: TimestampNs::new(timestamp),
        raw_ref: Some(RawEvidenceRef::new(raw_ref)),
        confidence_hint: ConfidenceHint::High,
        metadata,
    }
}

pub fn format_endpoint(addr: IpAddr, port: u16) -> String {
    match addr {
        IpAddr::V4(addr) => format!("{addr}:{port}"),
        IpAddr::V6(addr) => format!("[{addr}]:{port}"),
    }
}

fn malformed(reason: String) -> EbpfError {
    EbpfError::Malformed { reason }
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    let mut buf = [0u8; 2];
    buf.copy_from_slice(&bytes[offset..offset + 2]);
    u16::from_le_bytes(buf)
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

fn read_addr(bytes: &[u8], family: u8) -> Result<IpAddr, EbpfError> {
    if bytes.len() != ADDR_LEN {
        return Err(malformed(format!(
            "address is {} bytes, expected {ADDR_LEN}",
            bytes.len()
        )));
    }
    match family {
        4 => {
            let mut octets = [0u8; 4];
            octets.copy_from_slice(&bytes[..4]);
            Ok(IpAddr::V4(Ipv4Addr::from(octets)))
        }
        6 => {
            let mut octets = [0u8; 16];
            octets.copy_from_slice(bytes);
            Ok(IpAddr::V6(Ipv6Addr::from(octets)))
        }
        other => Err(malformed(format!("address family {other}"))),
    }
}

fn write_u16(out: &mut [u8], offset: usize, value: u16) {
    out[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(out: &mut [u8], offset: usize, value: u32) {
    out[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn write_u64(out: &mut [u8], offset: usize, value: u64) {
    out[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

fn write_addr(dst: &mut [u8], addr: IpAddr) -> u8 {
    dst.fill(0);
    match addr {
        IpAddr::V4(addr) => {
            dst[..4].copy_from_slice(&addr.octets());
            4
        }
        IpAddr::V6(addr) => {
            dst.copy_from_slice(&addr.octets());
            6
        }
    }
}
