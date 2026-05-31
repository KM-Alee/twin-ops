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
