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

    let (scan_quality, scan_quality_error) = if db_path.exists() {
        match twin_store::Store::open(&db_path) {
            Ok(store) if store.is_initialized().unwrap_or(false) => {
                match super::scan_quality::assess_scan_quality(&store) {
                    Ok(assessment) => (Some(assessment), None),
                    Err(error) => (None, Some(error.to_string())),
                }
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };

    Ok(DoctorResult {
        core: core::check(&config_path),
        database: database::check(&db_path),
        permissions: permissions::check(),
        scan_quality,
        scan_quality_error,
    })
}

pub fn run_home(config_override: Option<&Path>) -> Result<DoctorResult, AppError> {
    let layout = TwinLayout::from_xdg().map_err(AppError::Paths)?;
    run(&layout, config_override)
}
