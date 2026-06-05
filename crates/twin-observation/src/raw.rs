use twin_core::{CollectorName, NodeKind, TimestampNs};

use crate::observation::ObservationMetadata;
use crate::vocab::{ConfidenceHint, ObservationKind, ObservationSource};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RawIdentity {
    Host { hostname: String },
    Process { pid: u32 },
    Service { unit: String },
    TcpEndpoint { ip: String, port: u16 },
    UnixSocket { path: String },
    File { path: String },
    Cgroup { path: String },
}

impl RawIdentity {
    pub fn node_kind(&self) -> NodeKind {
        match self {
            Self::Host { .. } => NodeKind::Host,
            Self::Process { .. } => NodeKind::Process,
            Self::Service { .. } => NodeKind::Service,
            Self::TcpEndpoint { .. } => NodeKind::Port,
            Self::UnixSocket { .. } => NodeKind::UnixSocket,
            Self::File { .. } => NodeKind::File,
            Self::Cgroup { .. } => NodeKind::Cgroup,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEvidenceRef(String);

impl RawEvidenceRef {
    pub fn new(reference: impl Into<String>) -> Self {
        Self(reference.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone)]
pub struct RawObservation {
    pub source: ObservationSource,
    pub kind: ObservationKind,
    pub collector: CollectorName,
    pub subject: Option<RawIdentity>,
    pub object: Option<RawIdentity>,
    pub timestamp: TimestampNs,
    pub raw_ref: Option<RawEvidenceRef>,
    pub confidence_hint: ConfidenceHint,
    pub metadata: ObservationMetadata,
}
