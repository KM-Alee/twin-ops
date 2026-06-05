use std::path::PathBuf;

use serde::Serialize;

use super::ScanQualityAssessment;

#[derive(Debug, Clone, Serialize)]
pub struct DoctorResult {
    pub core: DoctorCore,
    pub database: DoctorDatabase,
    pub permissions: DoctorPermissions,
    pub scan_quality: Option<ScanQualityAssessment>,
    pub scan_quality_error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorCore {
    pub cli_ok: bool,
    pub config_found: bool,
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorDatabase {
    pub initialized: bool,
    pub schema_version: Option<i64>,
    pub db_path: Option<PathBuf>,
    pub wal_mode: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorPermissions {
    pub mode: PermissionMode,
    pub proc_accessible: bool,
    pub readable_process_count: Option<usize>,
    pub restricted_process_count: Option<usize>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    Privileged,
    Unprivileged,
    Partial,
}
