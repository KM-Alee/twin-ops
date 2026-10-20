use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use twin_core::ParseError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationSource {
    Containerd,
    Proc,
    ProcNetTcp,
    ProcNetUdp,
    ProcNetUnix,
    ProcCgroup,
    SystemdUnitFile,
    SystemdDBus,
    SystemdEnableSymlink,
    ConfigFileDiscovery,
    CronFile,
    SystemDns,
    Ebpf,
}

impl fmt::Display for ObservationSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Containerd => "containerd",
            Self::Proc => "proc",
            Self::ProcNetTcp => "proc_net_tcp",
            Self::ProcNetUdp => "proc_net_udp",
            Self::ProcNetUnix => "proc_net_unix",
            Self::ProcCgroup => "proc_cgroup",
            Self::SystemdUnitFile => "systemd_unit_file",
            Self::SystemdDBus => "systemd_dbus",
            Self::SystemdEnableSymlink => "systemd_enable_symlink",
            Self::ConfigFileDiscovery => "config_file_discovery",
            Self::CronFile => "cron_file",
            Self::SystemDns => "system_dns",
            Self::Ebpf => "ebpf",
        })
    }
}

impl FromStr for ObservationSource {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "containerd" => Ok(Self::Containerd),
            "proc" => Ok(Self::Proc),
            "proc_net_tcp" => Ok(Self::ProcNetTcp),
            "proc_net_udp" => Ok(Self::ProcNetUdp),
            "proc_net_unix" => Ok(Self::ProcNetUnix),
            "proc_cgroup" => Ok(Self::ProcCgroup),
            "systemd_unit_file" => Ok(Self::SystemdUnitFile),
            "systemd_dbus" => Ok(Self::SystemdDBus),
            "systemd_enable_symlink" => Ok(Self::SystemdEnableSymlink),
            "config_file_discovery" => Ok(Self::ConfigFileDiscovery),
            "cron_file" => Ok(Self::CronFile),
            "system_dns" => Ok(Self::SystemDns),
            "ebpf" => Ok(Self::Ebpf),
            other => Err(ParseError::Enum {
                kind: "ObservationSource",
                value: other.to_string(),
            }),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ObservationKind {
    ContainerSeen,
    ContainerTaskSeen,
    ContainerImageSeen,
    ProcessSeen,
    ProcessCommandSeen,
    ProcessExeSeen,
    ProcessParentSeen,
    ProcessSocketFdSeen,
    ProcessFifoFdSeen,
    TcpSocketSeen,
    UdpSocketSeen,
    TcpConnectionSeen,
    UdpConnectionSeen,
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
    SystemdTimerActivates,
    CronJobSeen,
    SystemdCgroupCorrection,
    ConfigFileSeen,
    ServiceConfiguredByFile,
    ConfigProxyPass,
    DnsAddressResolved,
    EbpfExecObserved,
    EbpfConnect,
    EbpfAccept,
    EbpfBind,
    EbpfDroppedEvents,
}

impl fmt::Display for ObservationKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::ContainerSeen => "container_seen",
            Self::ContainerTaskSeen => "container_task_seen",
            Self::ContainerImageSeen => "container_image_seen",
            Self::ProcessSeen => "process_seen",
            Self::ProcessCommandSeen => "process_command_seen",
            Self::ProcessExeSeen => "process_exe_seen",
            Self::ProcessParentSeen => "process_parent_seen",
            Self::ProcessSocketFdSeen => "process_socket_fd_seen",
            Self::ProcessFifoFdSeen => "process_fifo_fd_seen",
            Self::TcpSocketSeen => "tcp_socket_seen",
            Self::UdpSocketSeen => "udp_socket_seen",
            Self::TcpConnectionSeen => "tcp_connection_seen",
            Self::UdpConnectionSeen => "udp_connection_seen",
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
            Self::SystemdTimerActivates => "systemd_timer_activates",
            Self::CronJobSeen => "cron_job_seen",
            Self::SystemdCgroupCorrection => "systemd_cgroup_correction",
            Self::ConfigFileSeen => "config_file_seen",
            Self::ServiceConfiguredByFile => "service_configured_by_file",
            Self::ConfigProxyPass => "config_proxy_pass",
            Self::DnsAddressResolved => "dns_address_resolved",
            Self::EbpfExecObserved => "ebpf_exec_observed",
            Self::EbpfConnect => "ebpf_connect",
            Self::EbpfAccept => "ebpf_accept",
            Self::EbpfBind => "ebpf_bind",
            Self::EbpfDroppedEvents => "ebpf_dropped_events",
        })
    }
}

impl FromStr for ObservationKind {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "container_seen" => Ok(Self::ContainerSeen),
            "container_task_seen" => Ok(Self::ContainerTaskSeen),
            "container_image_seen" => Ok(Self::ContainerImageSeen),
            "process_seen" => Ok(Self::ProcessSeen),
            "process_command_seen" => Ok(Self::ProcessCommandSeen),
            "process_exe_seen" => Ok(Self::ProcessExeSeen),
            "process_parent_seen" => Ok(Self::ProcessParentSeen),
            "process_socket_fd_seen" => Ok(Self::ProcessSocketFdSeen),
            "process_fifo_fd_seen" => Ok(Self::ProcessFifoFdSeen),
            "tcp_socket_seen" => Ok(Self::TcpSocketSeen),
            "udp_socket_seen" => Ok(Self::UdpSocketSeen),
            "tcp_connection_seen" => Ok(Self::TcpConnectionSeen),
            "udp_connection_seen" => Ok(Self::UdpConnectionSeen),
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
            "systemd_timer_activates" => Ok(Self::SystemdTimerActivates),
            "cron_job_seen" => Ok(Self::CronJobSeen),
            "systemd_cgroup_correction" => Ok(Self::SystemdCgroupCorrection),
            "config_file_seen" => Ok(Self::ConfigFileSeen),
            "service_configured_by_file" => Ok(Self::ServiceConfiguredByFile),
            "config_proxy_pass" => Ok(Self::ConfigProxyPass),
            "dns_address_resolved" => Ok(Self::DnsAddressResolved),
            "ebpf_exec_observed" => Ok(Self::EbpfExecObserved),
            "ebpf_connect" => Ok(Self::EbpfConnect),
            "ebpf_accept" => Ok(Self::EbpfAccept),
            "ebpf_bind" => Ok(Self::EbpfBind),
            "ebpf_dropped_events" => Ok(Self::EbpfDroppedEvents),
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
