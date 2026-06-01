use twin_collectors::CollectorError;
use twin_core::error::ParseError;
use twin_core::NodeId;
use twin_observation::ObservationError;
use twin_store::{StoreError, StoreOpenError};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("init failed: {0}")]
    Init(#[from] InitError),
    #[error("path resolution failed: {0}")]
    Paths(#[from] PathError),
    #[error("scan failed: {0}")]
    Scan(#[from] ScanError),
    #[error("graph failed: {0}")]
    Graph(#[from] GraphError),
    #[error("collector error: {0}")]
    Collector(#[from] CollectorError),
    #[error("invalid graph target `{value}`: {source}")]
    InvalidGraphTarget {
        value: String,
        #[source]
        source: ParseError,
    },
    #[error("unsupported graph kind: {kind}")]
    UnsupportedGraphKind { kind: String },
}

#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("cannot resolve home directory")]
    NoHome,
}

#[derive(Debug, thiserror::Error)]
pub enum InitError {
    #[error("cannot create directories: {0}")]
    Directory(#[source] std::io::Error),
    #[error("cannot write config: {0}")]
    ConfigWrite(#[from] twin_core::error::ConfigError),
    #[error("cannot open database: {0}")]
    Database(#[from] StoreOpenError),
    #[error("cannot initialize database: {0}")]
    Migration(#[from] StoreError),
}

#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    #[error("path resolution failed: {0}")]
    Paths(#[from] PathError),
    #[error("database is not initialized; run `twin init` first")]
    DatabaseNotInitialized,
    #[error("cannot open database: {0}")]
    StoreOpen(#[from] StoreOpenError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("observation pipeline error: {0}")]
    Observation(#[from] ObservationError),
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("path resolution failed: {0}")]
    Paths(#[from] PathError),
    #[error("database is not initialized; run `twin init` first")]
    DatabaseNotInitialized,
    #[error("specify a node id or kind")]
    MissingQuery,
    #[error("cannot open database: {0}")]
    StoreOpen(#[from] StoreOpenError),
    #[error("store error: {0}")]
    Store(#[from] StoreError),
    #[error("node not found: {id}")]
    NodeNotFound { id: NodeId },
}
