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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ebpf: Option<DoctorEbpf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub containers: Option<DoctorContainers>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorContainers {
    pub socket_present: bool,
    pub socket_path: String,
    pub listed: bool,
    pub container_count: Option<usize>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorEbpf {
    pub kernel: DoctorEbpfCheck,
    pub btf: DoctorEbpfCheck,
    pub capabilities: DoctorEbpfCheck,
    pub exec_tracing: DoctorEbpfCheck,
    pub tcp_tracing: DoctorEbpfCheck,
}

#[derive(Debug, Clone, Serialize)]
pub struct DoctorEbpfCheck {
    pub ok: bool,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
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
