use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct InitResult {
    pub config_path: PathBuf,
    pub db_path: PathBuf,
    pub log_path: PathBuf,
    pub config_created: bool,
    pub config_updated: bool,
    pub db_created: bool,
}
