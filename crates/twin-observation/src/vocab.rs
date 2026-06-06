use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use twin_core::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationSource {
    Proc,
    ProcNetTcp,
    ProcNetUnix,
    ProcCgroup,
    SystemdUnitFile,
    SystemdDBus,
    SystemdEnableSymlink,
    ConfigFileDiscovery,
}

impl fmt::Display for ObservationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Proc => "proc",
            Self::ProcNetTcp => "proc_net_tcp",
            Self::ProcNetUnix => "proc_net_unix",
            Self::ProcCgroup => "proc_cgroup",
            Self::SystemdUnitFile => "systemd_unit_file",
            Self::SystemdDBus => "systemd_dbus",
            Self::SystemdEnableSymlink => "systemd_enable_symlink",
            Self::ConfigFileDiscovery => "config_file_discovery",
        })
    }
}

impl FromStr for ObservationSource {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "proc" => Ok(Self::Proc),
            "proc_net_tcp" => Ok(Self::ProcNetTcp),
            "proc_net_unix" => Ok(Self::ProcNetUnix),
            "proc_cgroup" => Ok(Self::ProcCgroup),
            "systemd_unit_file" => Ok(Self::SystemdUnitFile),
            "systemd_dbus" => Ok(Self::SystemdDBus),
            "systemd_enable_symlink" => Ok(Self::SystemdEnableSymlink),
            "config_file_discovery" => Ok(Self::ConfigFileDiscovery),
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
    UnixSocketSeen,
    UnixConnectionSeen,
    ProcessBelongsToCgroup,
    SystemdUnitSeen,
    SystemdUnitRequires,
    SystemdUnitWants,
    SystemdUnitStateSeen,
    SystemdUnitWantedBy,
    SystemdSocketSeen,
    SystemdSocketActivates,
    SystemdCgroupCorrection,
    ConfigFileSeen,
    ServiceConfiguredByFile,
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
            Self::UnixSocketSeen => "unix_socket_seen",
            Self::UnixConnectionSeen => "unix_connection_seen",
            Self::ProcessBelongsToCgroup => "process_belongs_to_cgroup",
            Self::SystemdUnitSeen => "systemd_unit_seen",
            Self::SystemdUnitRequires => "systemd_unit_requires",
            Self::SystemdUnitWants => "systemd_unit_wants",
            Self::SystemdUnitStateSeen => "systemd_unit_state_seen",
            Self::SystemdUnitWantedBy => "systemd_unit_wanted_by",
            Self::SystemdSocketSeen => "systemd_socket_seen",
            Self::SystemdSocketActivates => "systemd_socket_activates",
            Self::SystemdCgroupCorrection => "systemd_cgroup_correction",
            Self::ConfigFileSeen => "config_file_seen",
            Self::ServiceConfiguredByFile => "service_configured_by_file",
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
            "unix_socket_seen" => Ok(Self::UnixSocketSeen),
            "unix_connection_seen" => Ok(Self::UnixConnectionSeen),
            "process_belongs_to_cgroup" => Ok(Self::ProcessBelongsToCgroup),
            "systemd_unit_seen" => Ok(Self::SystemdUnitSeen),
            "systemd_unit_requires" => Ok(Self::SystemdUnitRequires),
            "systemd_unit_wants" => Ok(Self::SystemdUnitWants),
            "systemd_unit_state_seen" => Ok(Self::SystemdUnitStateSeen),
            "systemd_unit_wanted_by" => Ok(Self::SystemdUnitWantedBy),
            "systemd_socket_seen" => Ok(Self::SystemdSocketSeen),
            "systemd_socket_activates" => Ok(Self::SystemdSocketActivates),
            "systemd_cgroup_correction" => Ok(Self::SystemdCgroupCorrection),
            "config_file_seen" => Ok(Self::ConfigFileSeen),
            "service_configured_by_file" => Ok(Self::ServiceConfiguredByFile),
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
