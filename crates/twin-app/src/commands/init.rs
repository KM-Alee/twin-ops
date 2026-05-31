use twin_store::Store;

use crate::config_io;
use crate::error::{AppError, InitError};
use crate::model::InitResult;
use crate::{InitRequest, TwinLayout};

pub fn run(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, AppError> {
    run_inner(layout, request).map_err(AppError::Init)
}

fn run_inner(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, InitError> {
    let cfg_path = layout.config_file(request.config_override.as_deref());
    let db_path = layout.db_file();
    let log_path = layout.log_file();

    layout.ensure_dirs().map_err(InitError::Directory)?;

    let (config_created, config_updated) =
        config_io::sync_default_template(&cfg_path, request.force)
            .map_err(InitError::ConfigWrite)?;

    let db_was_initialized = db_path.exists()
        && Store::open(&db_path)
            .ok()
            .is_some_and(|store| store.is_initialized());

    let store = Store::open(&db_path).map_err(InitError::Database)?;
    if !db_was_initialized {
        store.initialize().map_err(InitError::Migration)?;
    }

    Ok(InitResult {
        config_path: cfg_path,
        db_path,
        log_path,
        config_created,
        config_updated,
        db_created: !db_was_initialized,
    })
}

pub fn run_home(request: InitRequest) -> Result<InitResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    run(&layout, request)
}
