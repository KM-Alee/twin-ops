use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ContainerdError {
    #[error("containerd {operation} unavailable: {category}")]
    Rpc {
        operation: &'static str,
        category: String,
    },
    #[error("cannot read containerd fixture {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid containerd {resource}: {detail}")]
    Invalid {
        resource: &'static str,
        detail: &'static str,
    },
}
