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
}
