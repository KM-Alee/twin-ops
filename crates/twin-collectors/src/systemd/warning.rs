use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemdWarningKind {
    PathUnreadable,
    ParseError,
    GlobInstanceSkipped,
    MaskedEmpty,
    DbusUnavailable,
}

impl SystemdWarningKind {
    pub fn aggregate_key(self) -> &'static str {
        match self {
            Self::PathUnreadable => "systemd_path_unreadable",
            Self::ParseError => "systemd_parse_error",
            Self::GlobInstanceSkipped => "systemd_glob_skipped",
            Self::MaskedEmpty => "systemd_masked_empty",
            Self::DbusUnavailable => "systemd_dbus_unavailable",
        }
    }

    pub fn from_aggregate_key(key: &str) -> Option<Self> {
        match key {
            "systemd_path_unreadable" => Some(Self::PathUnreadable),
            "systemd_parse_error" => Some(Self::ParseError),
            "systemd_glob_skipped" => Some(Self::GlobInstanceSkipped),
            "systemd_masked_empty" => Some(Self::MaskedEmpty),
            "systemd_dbus_unavailable" => Some(Self::DbusUnavailable),
            _ => None,
        }
    }

    pub fn cli_summary(self, count: usize) -> (&'static str, String) {
        match self {
            Self::PathUnreadable => ("systemd", format!("{count} unit paths unreadable")),
            Self::ParseError => ("systemd", format!("{count} unit files failed to parse")),
            Self::GlobInstanceSkipped => (
                "systemd",
                format!("{count} template/instance units skipped"),
            ),
            Self::MaskedEmpty => ("systemd", format!("{count} empty unit files skipped")),
            Self::DbusUnavailable => ("systemd", format!("{count} D-Bus unavailable")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemdWarning {
    kind: SystemdWarningKind,
    path: PathBuf,
    detail: String,
}

impl SystemdWarning {
    pub(crate) fn new(kind: SystemdWarningKind, path: PathBuf, detail: impl Into<String>) -> Self {
        Self {
            kind,
            path,
            detail: detail.into(),
        }
    }

    pub fn kind(&self) -> SystemdWarningKind {
        self.kind
    }

    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn detail(&self) -> &str {
        &self.detail
    }
}
