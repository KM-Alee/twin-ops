use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use twin_core::{CollectorName, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, RawEvidenceRef,
    RawIdentity, RawObservation,
};

use crate::error::EbpfError;

pub const TCP_CONNECT_LEN: usize = 56;
pub const TCP_CONNECT_LIVE_LEN: usize = 72;
pub const TCP_ACCEPT_LEN: usize = 58;
pub const TCP_ACCEPT_LIVE_LEN: usize = 74;
pub const TCP_BIND_LEN: usize = 40;
pub const TCP_BIND_LIVE_LEN: usize = 72;
const ADDR_LEN: usize = 16;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpConnect {
    pid: u32,
    cgroup_id: u64,
    saddr: IpAddr,
    daddr: IpAddr,
    dport: u16,
    timestamp_ns: u64,
    namespace: Option<twin_core::NetworkNamespaceId>,
    start_ticks: Option<u64>,
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
            namespace: None,
            start_ticks: None,
        }
    }

    pub fn network_namespace(&self) -> Option<twin_core::NetworkNamespaceId> {
        self.namespace
    }
    pub fn start_ticks(&self) -> Option<u64> {
        self.start_ticks
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
        let endpoint = format_endpoint(self.daddr, self.dport);
        match self.namespace {
            Some(namespace) => format!("{endpoint} (netns:{namespace})"),
            None => endpoint,
        }
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
    namespace: Option<twin_core::NetworkNamespaceId>,
    start_ticks: Option<u64>,
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
            namespace: None,
            start_ticks: None,
        }
    }

    pub fn network_namespace(&self) -> Option<twin_core::NetworkNamespaceId> {
        self.namespace
    }
    pub fn start_ticks(&self) -> Option<u64> {
        self.start_ticks
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
        scoped_endpoint(self.saddr, self.sport, self.namespace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpBind {
    pid: u32,
    cgroup_id: u64,
    addr: IpAddr,
    port: u16,
    timestamp_ns: u64,
    namespace: Option<twin_core::NetworkNamespaceId>,
    start_ticks: Option<u64>,
}

impl TcpBind {
    pub fn new(pid: u32, cgroup_id: u64, addr: IpAddr, port: u16, timestamp_ns: u64) -> Self {
        Self {
            pid,
            cgroup_id,
            addr,
            port,
            timestamp_ns,
            namespace: None,
            start_ticks: None,
        }
    }

    pub fn network_namespace(&self) -> Option<twin_core::NetworkNamespaceId> {
        self.namespace
    }
    pub fn start_ticks(&self) -> Option<u64> {
        self.start_ticks
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
        scoped_endpoint(self.addr, self.port, self.namespace)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TcpEvent {
    Connect(TcpConnect),
    Accept(TcpAccept),
    Bind(TcpBind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TcpEventKind {
    Connect,
    Accept,
    Bind,
}

impl TcpEvent {
    pub fn start_ticks(&self) -> Option<u64> {
        match self {
            Self::Connect(event) => event.start_ticks(),
            Self::Accept(event) => event.start_ticks(),
            Self::Bind(event) => event.start_ticks(),
        }
    }

    pub fn kind(&self) -> TcpEventKind {
        match self {
            Self::Connect(_) => TcpEventKind::Connect,
            Self::Accept(_) => TcpEventKind::Accept,
            Self::Bind(_) => TcpEventKind::Bind,
        }
    }
    pub fn cgroup_id(&self) -> u64 {
        match self {
            Self::Connect(event) => event.cgroup_id(),
            Self::Accept(event) => event.cgroup_id(),
            Self::Bind(event) => event.cgroup_id(),
        }
    }

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
        TCP_BIND_LIVE_LEN if bytes[23] == 2 => decode_bind(bytes).map(TcpEvent::Bind),
        TCP_CONNECT_LEN | TCP_CONNECT_LIVE_LEN => decode_connect(bytes).map(TcpEvent::Connect),
        TCP_ACCEPT_LEN | TCP_ACCEPT_LIVE_LEN => decode_accept(bytes).map(TcpEvent::Accept),
        TCP_BIND_LEN => decode_bind(bytes).map(TcpEvent::Bind),
        other => Err(malformed(format!(
            "tcp event is {other} bytes, expected {TCP_CONNECT_LEN}, {TCP_ACCEPT_LEN}, or {TCP_BIND_LEN}"
        ))),
    }
}

pub fn decode_connect(bytes: &[u8]) -> Result<TcpConnect, EbpfError> {
    if ![TCP_CONNECT_LEN, TCP_CONNECT_LIVE_LEN].contains(&bytes.len()) {
        return Err(malformed(format!(
            "connect event is {} bytes, expected {TCP_CONNECT_LEN}",
            bytes.len()
        )));
    }
    if bytes[23] != 0 {
        return Err(malformed(
            "connect event has unsupported kind marker".into(),
        ));
    }
    let family = bytes[22];
    Ok(TcpConnect {
        pid: read_u32(bytes, 0),
        cgroup_id: read_u64(bytes, 4),
        timestamp_ns: read_u64(bytes, 12),
        dport: read_u16(bytes, 20),
        saddr: read_addr(&bytes[24..40], family)?,
        daddr: read_addr(&bytes[40..56], family)?,
        namespace: if bytes.len() == TCP_CONNECT_LIVE_LEN {
            Some(
                twin_core::NetworkNamespaceId::new(read_u64(bytes, 56))
                    .map_err(|_| malformed("live event has invalid namespace".into()))?,
            )
        } else {
            None
        },
        start_ticks: if bytes.len() == TCP_CONNECT_LIVE_LEN {
            Some(read_u64(bytes, 64))
        } else {
            None
        },
    })
}

pub fn decode_accept(bytes: &[u8]) -> Result<TcpAccept, EbpfError> {
    if ![TCP_ACCEPT_LEN, TCP_ACCEPT_LIVE_LEN].contains(&bytes.len()) {
        return Err(malformed(format!(
            "accept event is {} bytes, expected {TCP_ACCEPT_LEN}",
            bytes.len()
        )));
    }
    if bytes[25] != 0 {
        return Err(malformed("accept event has unsupported kind marker".into()));
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
        namespace: live_namespace(bytes, TCP_ACCEPT_LIVE_LEN, 58)?,
        start_ticks: (bytes.len() == TCP_ACCEPT_LIVE_LEN).then(|| read_u64(bytes, 66)),
    })
}

pub fn decode_bind(bytes: &[u8]) -> Result<TcpBind, EbpfError> {
    if ![TCP_BIND_LEN, TCP_BIND_LIVE_LEN].contains(&bytes.len()) {
        return Err(malformed(format!(
            "bind event is {} bytes, expected {TCP_BIND_LEN}",
            bytes.len()
        )));
    }
    if bytes[23]
        != if bytes.len() == TCP_BIND_LIVE_LEN {
            2
        } else {
            0
        }
    {
        return Err(malformed("bind event has unsupported kind marker".into()));
    }
    let family = bytes[22];
    Ok(TcpBind {
        pid: read_u32(bytes, 0),
        cgroup_id: read_u64(bytes, 4),
        timestamp_ns: read_u64(bytes, 12),
        port: read_u16(bytes, 20),
        addr: read_addr(&bytes[24..40], family)?,
        namespace: live_namespace(bytes, TCP_BIND_LIVE_LEN, 56)?,
        start_ticks: (bytes.len() == TCP_BIND_LIVE_LEN).then(|| read_u64(bytes, 64)),
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
    let mut raw = observation(
        ObservationKind::EbpfConnect,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.daddr().to_string(),
            port: event.dport(),
        }),
        format!("ebpf:tcp:connect:{}:{endpoint}", event.pid()),
    );
    if let Some(namespace) = event.network_namespace() {
        raw.object = Some(RawIdentity::NamespacedEndpoint {
            namespace,
            protocol: twin_core::TransportProtocol::Tcp,
            ip: event.daddr().to_string(),
            port: event.dport(),
        });
        raw.metadata
            .insert_str("network_namespace", &namespace.to_string());
    }
    if let Some(start) = event.start_ticks() {
        raw.metadata.insert_u64("start_ticks", start);
    }
    raw
}

fn accept_raw(event: &TcpAccept) -> RawObservation {
    let endpoint = event.endpoint();
    let mut raw = observation(
        ObservationKind::EbpfAccept,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.saddr().to_string(),
            port: event.sport(),
        }),
        format!("ebpf:tcp:accept:{}:{endpoint}", event.pid()),
    );
    attach_identity(
        &mut raw,
        event.saddr(),
        event.sport(),
        event.network_namespace(),
        event.start_ticks(),
    );
    raw
}

fn bind_raw(event: &TcpBind) -> RawObservation {
    let endpoint = event.endpoint();
    let mut raw = observation(
        ObservationKind::EbpfBind,
        event.pid(),
        event.cgroup_id(),
        event.timestamp_ns(),
        Some(RawIdentity::TcpEndpoint {
            ip: event.addr().to_string(),
            port: event.port(),
        }),
        format!("ebpf:tcp:bind:{}:{endpoint}", event.pid()),
    );
    attach_identity(
        &mut raw,
        event.addr(),
        event.port(),
        event.network_namespace(),
        event.start_ticks(),
    );
    raw
}

fn live_namespace(
    bytes: &[u8],
    live_len: usize,
    offset: usize,
) -> Result<Option<twin_core::NetworkNamespaceId>, EbpfError> {
    if bytes.len() != live_len {
        return Ok(None);
    }
    twin_core::NetworkNamespaceId::new(read_u64(bytes, offset))
        .map(Some)
        .map_err(|_| malformed("live event has invalid namespace".into()))
}

fn scoped_endpoint(
    addr: IpAddr,
    port: u16,
    namespace: Option<twin_core::NetworkNamespaceId>,
) -> String {
    let endpoint = format_endpoint(addr, port);
    match namespace {
        Some(namespace) => format!("{endpoint} (netns:{namespace})"),
        None => endpoint,
    }
}

fn attach_identity(
    raw: &mut RawObservation,
    addr: IpAddr,
    port: u16,
    namespace: Option<twin_core::NetworkNamespaceId>,
    start_ticks: Option<u64>,
) {
    if let Some(namespace) = namespace {
        raw.object = Some(RawIdentity::NamespacedEndpoint {
            namespace,
            protocol: twin_core::TransportProtocol::Tcp,
            ip: addr.to_string(),
            port,
        });
        raw.metadata
            .insert_str("network_namespace", &namespace.to_string());
    }
    if let Some(start) = start_ticks {
        raw.metadata.insert_u64("start_ticks", start);
    }
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
