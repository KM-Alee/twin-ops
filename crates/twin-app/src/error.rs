use twin_core::error::ConfigError;
use twin_store::{StoreError, StoreOpenError};

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("init failed: {0}")]
    Init(#[from] InitError),
    #[error("path resolution failed: {0}")]
    Paths(#[from] PathError),
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
    ConfigWrite(#[from] ConfigError),
    #[error("cannot open database: {0}")]
    Database(#[from] StoreOpenError),
    #[error("cannot initialize database: {0}")]
    Migration(#[from] StoreError),
}
