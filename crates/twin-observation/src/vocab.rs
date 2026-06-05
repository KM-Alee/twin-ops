use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use twin_core::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationSource {
    Proc,
    ProcNetTcp,
    ProcCgroup,
}

impl fmt::Display for ObservationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Proc => "proc",
            Self::ProcNetTcp => "proc_net_tcp",
            Self::ProcCgroup => "proc_cgroup",
        })
    }
}

impl FromStr for ObservationSource {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "proc" => Ok(Self::Proc),
            "proc_net_tcp" => Ok(Self::ProcNetTcp),
            "proc_cgroup" => Ok(Self::ProcCgroup),
            other => Err(ParseError::Enum {
                kind: "ObservationSource",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationKind {
    ProcessSeen,
    ProcessCommandSeen,
    ProcessExeSeen,
    ProcessParentSeen,
    TcpSocketSeen,
    TcpConnectionSeen,
    ProcessBelongsToCgroup,
}

impl fmt::Display for ObservationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ProcessSeen => "process_seen",
            Self::ProcessCommandSeen => "process_command_seen",
            Self::ProcessExeSeen => "process_exe_seen",
            Self::ProcessParentSeen => "process_parent_seen",
            Self::TcpSocketSeen => "tcp_socket_seen",
            Self::TcpConnectionSeen => "tcp_connection_seen",
            Self::ProcessBelongsToCgroup => "process_belongs_to_cgroup",
        })
    }
}

impl FromStr for ObservationKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "process_seen" => Ok(Self::ProcessSeen),
            "process_command_seen" => Ok(Self::ProcessCommandSeen),
            "process_exe_seen" => Ok(Self::ProcessExeSeen),
            "process_parent_seen" => Ok(Self::ProcessParentSeen),
            "tcp_socket_seen" => Ok(Self::TcpSocketSeen),
            "tcp_connection_seen" => Ok(Self::TcpConnectionSeen),
            "process_belongs_to_cgroup" => Ok(Self::ProcessBelongsToCgroup),
            other => Err(ParseError::Enum {
                kind: "ObservationKind",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ConfidenceHint {
    Low,
    Moderate,
    High,
}

impl fmt::Display for ConfidenceHint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Low => "low",
            Self::Moderate => "moderate",
            Self::High => "high",
        })
    }
}

impl FromStr for ConfidenceHint {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "low" => Ok(Self::Low),
            "moderate" => Ok(Self::Moderate),
            "high" => Ok(Self::High),
            other => Err(ParseError::Enum {
                kind: "ConfidenceHint",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RedactionState {
    None,
    Partial,
    Redacted,
}

impl fmt::Display for RedactionState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::None => "none",
            Self::Partial => "partial",
            Self::Redacted => "redacted",
        })
    }
}

impl FromStr for RedactionState {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "none" => Ok(Self::None),
            "partial" => Ok(Self::Partial),
            "redacted" => Ok(Self::Redacted),
            other => Err(ParseError::Enum {
                kind: "RedactionState",
                value: other.to_string(),
            }),
        }
    }
}
