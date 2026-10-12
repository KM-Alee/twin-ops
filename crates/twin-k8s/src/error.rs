use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum K8sError {
    #[error("Kubernetes client unavailable: {detail}")]
    Client { detail: String },
    #[error("fixture directory is missing: {path}")]
    MissingFixture { path: PathBuf },
    #[error("cannot read {path}: {source}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
