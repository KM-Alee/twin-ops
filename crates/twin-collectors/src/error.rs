use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum CollectorError {
    #[error("cannot read proc root at {path}")]
    ProcRootRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid proc root at {path}")]
    InvalidProcRoot { path: PathBuf },
    #[error("systemd unit parse failed at {path}: {detail}")]
    SystemdParse { path: PathBuf, detail: String },
    #[error("systemd collector error: {detail}")]
    SystemdCollector { detail: String },
}

impl From<crate::systemd::SystemdDBusError> for CollectorError {
    fn from(value: crate::systemd::SystemdDBusError) -> Self {
        Self::SystemdCollector {
            detail: value.to_string(),
        }
    }
}
