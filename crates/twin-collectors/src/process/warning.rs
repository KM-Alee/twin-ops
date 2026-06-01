use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessWarningKind {
    Vanished,
    PermissionDenied,
    Malformed,
    ExeUnreadable,
    CgroupMissing,
    CgroupPermissionDenied,
    CgroupMalformed,
    TcpTableMissing,
    TcpTableMalformed,
    FdPermissionDenied,
    FdMalformed,
    FdVanished,
    SocketUnmapped,
}

impl ProcessWarningKind {
    pub fn aggregate_key(self) -> &'static str {
        match self {
            Self::Vanished => "vanished_processes",
            Self::PermissionDenied => "permission_denied",
            Self::Malformed => "malformed_proc_files",
            Self::ExeUnreadable => "exe_unreadable",
            Self::CgroupMissing => "cgroup_missing",
            Self::CgroupPermissionDenied => "cgroup_permission_denied",
            Self::CgroupMalformed => "cgroup_malformed",
            Self::TcpTableMissing => "tcp_table_missing",
            Self::TcpTableMalformed => "tcp_table_malformed",
            Self::FdPermissionDenied => "fd_permission_denied",
            Self::FdMalformed => "fd_malformed",
            Self::FdVanished => "fd_vanished",
            Self::SocketUnmapped => "socket_unmapped",
        }
    }

    pub fn detail_key(self) -> &'static str {
        match self {
            Self::Vanished => "vanished",
            Self::PermissionDenied => "permission_denied",
            Self::Malformed => "malformed",
            Self::ExeUnreadable => "exe_unreadable",
            Self::CgroupMissing => "cgroup_missing",
            Self::CgroupPermissionDenied => "cgroup_permission_denied",
            Self::CgroupMalformed => "cgroup_malformed",
            Self::TcpTableMissing => "tcp_table_missing",
            Self::TcpTableMalformed => "tcp_table_malformed",
            Self::FdPermissionDenied => "fd_permission_denied",
            Self::FdMalformed => "fd_malformed",
            Self::FdVanished => "fd_vanished",
            Self::SocketUnmapped => "socket_unmapped",
        }
    }

    pub fn includes_json_detail(self) -> bool {
        matches!(
            self,
            Self::Vanished
                | Self::Malformed
                | Self::CgroupMalformed
                | Self::TcpTableMalformed
                | Self::FdMalformed
                | Self::SocketUnmapped
        )
    }

    pub fn from_aggregate_key(key: &str) -> Option<Self> {
        match key {
            "vanished_processes" => Some(Self::Vanished),
            "permission_denied" => Some(Self::PermissionDenied),
            "malformed_proc_files" => Some(Self::Malformed),
            "exe_unreadable" => Some(Self::ExeUnreadable),
            "cgroup_missing" => Some(Self::CgroupMissing),
            "cgroup_permission_denied" => Some(Self::CgroupPermissionDenied),
            "cgroup_malformed" => Some(Self::CgroupMalformed),
            "tcp_table_missing" => Some(Self::TcpTableMissing),
            "tcp_table_malformed" => Some(Self::TcpTableMalformed),
            "fd_permission_denied" => Some(Self::FdPermissionDenied),
            "fd_malformed" => Some(Self::FdMalformed),
            "fd_vanished" => Some(Self::FdVanished),
            "socket_unmapped" => Some(Self::SocketUnmapped),
            _ => None,
        }
    }

    pub fn cli_summary(self, count: usize) -> (&'static str, String) {
        match self {
            Self::Vanished => ("vanished", format!("{count} disappeared during scan")),
            Self::ExeUnreadable => ("exe", format!("{count} unreadable exe links")),
            Self::PermissionDenied => ("permission", format!("{count} denied")),
            Self::Malformed => ("malformed", format!("{count} bad proc files")),
            Self::CgroupMissing => ("cgroup", format!("{count} missing cgroup files")),
            Self::CgroupPermissionDenied => ("cgroup", format!("{count} cgroup permission denied")),
            Self::CgroupMalformed => ("cgroup", format!("{count} malformed cgroup lines")),
            Self::TcpTableMissing => ("tcp", format!("{count} missing tcp tables")),
            Self::TcpTableMalformed => ("tcp", format!("{count} malformed tcp rows")),
            Self::FdPermissionDenied => (
                "fd",
                format!("{count} fd directories hidden by permissions"),
            ),
            Self::FdMalformed => ("fd", format!("{count} malformed fd entries")),
            Self::FdVanished => ("fd", format!("{count} vanished fd symlinks")),
            Self::SocketUnmapped => (
                "socket",
                format!("{count} listener sockets could not be mapped to processes"),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessWarning {
    kind: ProcessWarningKind,
    path: PathBuf,
    detail: String,
}

impl ProcessWarning {
    pub(crate) fn new(kind: ProcessWarningKind, path: PathBuf, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> ProcessWarningKind {
        self.kind
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}
