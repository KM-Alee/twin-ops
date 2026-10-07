use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum TestError {
    #[error("cannot read `{path}`: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot write `{path}`: {source}")]
    Write {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("`{path}` already exists; pass --force to replace it")]
    AlreadyExists { path: PathBuf },
    #[error("invalid test file `{path}`: {reason}")]
    Invalid { path: PathBuf, reason: String },
    #[error("shell execution is not supported in twin.yaml")]
    ShellNotSupported,
}
