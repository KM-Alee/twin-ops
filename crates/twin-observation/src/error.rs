#[derive(Debug, thiserror::Error)]
pub enum ObservationError {
    #[error("normalization failed: {0}")]
    Normalize(#[from] twin_core::ParseError),
    #[error("metadata is not a json object: {source}")]
    Metadata { source: serde_json::Error },
}
