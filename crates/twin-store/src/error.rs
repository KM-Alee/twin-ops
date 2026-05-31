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
    #[error("insert failed: {source}")]
    Insert { source: rusqlite::Error },
    #[error("upsert failed: {source}")]
    Upsert { source: rusqlite::Error },
    #[error("transaction begin failed: {source}")]
    TransactionBegin { source: rusqlite::Error },
    #[error("transaction commit failed: {source}")]
    TransactionCommit { source: rusqlite::Error },
    #[error("cannot decode stored observation: {detail}")]
    Decode { detail: String },
}

pub fn is_foreign_key_violation(err: &rusqlite::Error) -> bool {
    match err {
        rusqlite::Error::SqliteFailure(code, _) => {
            code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_FOREIGNKEY
        }
        _ => false,
    }
}

pub fn store_error_source(err: &StoreError) -> Option<&rusqlite::Error> {
    match err {
        StoreError::Insert { source }
        | StoreError::Upsert { source }
        | StoreError::Query { source }
        | StoreError::Migration { source, .. }
        | StoreError::HealthCheck { source }
        | StoreError::TransactionBegin { source }
        | StoreError::TransactionCommit { source } => Some(source),
        StoreError::Decode { .. } => None,
    }
}
