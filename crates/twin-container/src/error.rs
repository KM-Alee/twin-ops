use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ContainerError {
    #[error("docker data missing at {path}: {detail}")]
    Missing { path: PathBuf, detail: String },
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse docker json from {path}: {detail}")]
    Parse { path: String, detail: String },
    #[error("docker is unavailable: {detail}")]
    Unavailable { detail: String },
    #[error("docker api {path} returned {status}: {detail}")]
    Api {
        path: String,
        status: u16,
        detail: String,
    },
}
