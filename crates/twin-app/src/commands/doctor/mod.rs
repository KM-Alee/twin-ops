mod core;
mod database;
mod permissions;

use std::path::Path;

use crate::error::AppError;
use crate::model::DoctorResult;
use crate::paths::TwinLayout;

pub fn run(layout: &TwinLayout, config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    let config_path = layout.config_file(config_override);
    let db_path = layout.db_file();

    Ok(DoctorResult {
        core: core::check(&config_path),
        database: database::check(&db_path),
        permissions: permissions::check(),
    })
}

pub fn run_home(config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    run(&layout, config_override)
}
