use twin_core::{SnapshotId, TimestampNs};
use twin_store::{SnapshotError, Store, StoreError};

use crate::error::{AppError, TemporalError};
use crate::model::{SnapshotCreateResult, SnapshotEntry, SnapshotListResult};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::{SnapshotCreateRequest, SnapshotListRequest};

pub fn create_home(request: SnapshotCreateRequest) -> Result<SnapshotCreateResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    create_at(&paths.layout, &request, &paths.db_path)
}

pub fn create_in(
    layout: &TwinLayout,
    request: SnapshotCreateRequest,
) -> Result<SnapshotCreateResult, AppError> {
    create_at(layout, &request, &layout.db_file())
}

pub fn list_home(request: SnapshotListRequest) -> Result<SnapshotListResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    list_at(&paths.layout, &request, &paths.db_path)
}

pub fn list_in(
    layout: &TwinLayout,
    request: SnapshotListRequest,
) -> Result<SnapshotListResult, AppError> {
    list_at(layout, &request, &layout.db_file())
}

fn create_at(
    layout: &TwinLayout,
    request: &SnapshotCreateRequest,
    db_path: &std::path::Path,
) -> Result<SnapshotCreateResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let name =
        SnapshotId::new(&request.name).map_err(|source| TemporalError::InvalidSnapshotName {
            value: request.name.clone(),
            source,
        })?;
    let mut store = Store::open(db_path).map_err(TemporalError::StoreOpen)?;
    if !store.is_initialized().map_err(TemporalError::Store)? {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let created_at_ns = TimestampNs::now().as_i64();
    let row = store
        .create_snapshot(name.as_str(), created_at_ns)
        .map_err(map_snapshot_store_error)?;
    Ok(SnapshotCreateResult {
        name: row.name,
        created_at_ns: row.created_at_ns,
        node_count: row.node_count,
        edge_count: row.edge_count,
    })
}

fn list_at(
    layout: &TwinLayout,
    request: &SnapshotListRequest,
    db_path: &std::path::Path,
) -> Result<SnapshotListResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let store = Store::open(db_path).map_err(TemporalError::StoreOpen)?;
    if !store.is_initialized().map_err(TemporalError::Store)? {
        return Err(TemporalError::DatabaseNotInitialized.into());
    }
    let rows = store.list_snapshots().map_err(TemporalError::Store)?;
    Ok(SnapshotListResult {
        snapshots: rows
            .into_iter()
            .map(|row| SnapshotEntry {
                name: row.name,
                created_at_ns: row.created_at_ns,
                node_count: row.node_count,
                edge_count: row.edge_count,
            })
            .collect(),
    })
}

fn map_snapshot_store_error(err: StoreError) -> TemporalError {
    match err {
        StoreError::Snapshot(SnapshotError::DuplicateName { name }) => {
            TemporalError::DuplicateSnapshot { name }
        }
        StoreError::Snapshot(SnapshotError::EmptyGraph) => TemporalError::EmptyGraph,
        StoreError::Snapshot(SnapshotError::NotFound { name }) => {
            TemporalError::SnapshotNotFound { name }
        }
        other => TemporalError::Store(other),
    }
}
