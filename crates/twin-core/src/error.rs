#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("invalid observation id: {value}")]
    ObservationId { value: String },
    #[error("invalid ip address: {value}")]
    IpAddr { value: String },
    #[error("unknown {kind} variant: {value}")]
    Enum { kind: &'static str, value: String },
    #[error("invalid node id: {value}")]
    InvalidNodeId { value: String },
    #[error("invalid edge id: {value}")]
    InvalidEdgeId { value: String },
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("config read error: {path}: {source}")]
    Read {
        path: String,
        source: std::io::Error,
    },
    #[error("config parse error: {source}")]
    Parse { source: toml::de::Error },
    #[error("config write error: {path}: {source}")]
    Write {
        path: String,
        source: std::io::Error,
    },
}
