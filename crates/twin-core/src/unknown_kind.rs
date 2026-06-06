use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::error::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownKind {
    ScanHealth,
    MissingEvidence,
    UnmappedActiveSockets,
    UnmappedListenerSockets,
}

impl UnknownKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ScanHealth => "scan_health",
            Self::MissingEvidence => "missing_evidence",
            Self::UnmappedActiveSockets => "unmapped_active_sockets",
            Self::UnmappedListenerSockets => "unmapped_listener_sockets",
        }
    }

    pub fn human_label(self) -> &'static str {
        match self {
            Self::ScanHealth => "scan health",
            Self::MissingEvidence => "missing evidence",
            Self::UnmappedActiveSockets => "unmapped active sockets",
            Self::UnmappedListenerSockets => "unmapped listeners",
        }
    }
}

impl fmt::Display for UnknownKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for UnknownKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "scan_health" => Ok(Self::ScanHealth),
            "missing_evidence" => Ok(Self::MissingEvidence),
            "unmapped_active_sockets" => Ok(Self::UnmappedActiveSockets),
            "unmapped_listener_sockets" => Ok(Self::UnmappedListenerSockets),
            other => Err(ParseError::Enum {
                kind: "UnknownKind",
                value: other.to_string(),
            }),
        }
    }
}
