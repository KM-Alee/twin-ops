mod commands;
mod config_io;
mod error;
mod model;
pub mod paths;

pub use error::AppError;
pub use model::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, InitResult, PermissionMode,
};
pub use paths::TwinLayout;

use std::path::PathBuf;

#[derive(Debug, Clone, Default)]
pub struct InitRequest {
    pub force: bool,
    pub config_override: Option<PathBuf>,
}

pub fn init(request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run_home(request)
}

pub fn init_in(layout: &TwinLayout, request: InitRequest) -> Result<InitResult, AppError> {
    commands::init::run(layout, request)
}

pub fn doctor(config_override: Option<&std::path::Path>) -> Result<DoctorResult, AppError> {
    commands::doctor::run_home(config_override)
}

pub fn doctor_in(
    layout: &TwinLayout,
    config_override: Option<&std::path::Path>,
) -> Result<DoctorResult, AppError> {
    commands::doctor::run(layout, config_override)
}

pub fn read_config(
    path: &std::path::Path,
) -> Result<twin_core::config::TwinConfig, twin_core::error::ConfigError> {
    config_io::read(path)
}
