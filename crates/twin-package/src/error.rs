use std::path::PathBuf;
#[derive(Debug, thiserror::Error)]
pub enum PackageError {
    #[error("cannot create private RPM snapshot: {source}")]
    Snapshot { source: std::io::Error },
    #[error("cannot inspect RPM database {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("cannot read RPM SQLite database {path}: {source}")]
    Sqlite {
        path: PathBuf,
        source: rusqlite::Error,
    },
    #[error("RPM database {path} is unavailable: {detail}")]
    Unavailable { path: PathBuf, detail: &'static str },
}
