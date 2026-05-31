#[derive(Debug, thiserror::Error)]
pub enum StoreOpenError {
    #[error("cannot create parent directory for {path}: {source}")]
    CreateDir {
        path: String,
        source: std::io::Error,
    },
    #[error("cannot open database at {path}: {source}")]
    Open {
        path: String,
        source: rusqlite::Error,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("migration v{version} failed: {source}")]
    Migration {
        version: i64,
        source: rusqlite::Error,
    },
    #[error("database health check failed: {source}")]
    HealthCheck { source: rusqlite::Error },
    #[error("query failed: {source}")]
    Query { source: rusqlite::Error },
}
