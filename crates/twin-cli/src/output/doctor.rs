use twin_app::{DoctorResult, PermissionMode};

pub fn render(result: &DoctorResult) -> String {
    let config_status = if result.core.config_found {
        "OK"
    } else {
        "not found"
    };

    let database_status = if result.database.initialized {
        match result.database.schema_version {
            Some(v) => format!("initialized (schema v{v})"),
            None => "initialized".to_string(),
        }
    } else {
        "not initialized".to_string()
    };

    let wal_status = match result.database.wal_mode {
        Some(true) => "wal",
        Some(false) => "not wal",
        None => "unknown",
    };

    let permission_mode = match result.permissions.mode {
        PermissionMode::Privileged => "privileged",
        PermissionMode::Unprivileged => "unprivileged",
        PermissionMode::Partial => "partial",
    };

    let proc_line = if result.permissions.proc_accessible {
        match (
            result.permissions.readable_process_count,
            result.permissions.restricted_process_count,
        ) {
            (Some(readable), Some(restricted)) => {
                format!("accessible ({readable} readable, {restricted} restricted)")
            }
            _ => "accessible".to_string(),
        }
    } else {
        "not accessible".to_string()
    };

    let config_path = result
        .core
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let db_path = result
        .database
        .db_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());

    format!(
        "Twin Doctor\n\n\
         Core:\n\
         - CLI: OK\n\
         - Config: {config_status} ({config_path})\n\
         - Database: {database_status} ({db_path})\n\
         - WAL mode: {wal_status}\n\
         - Permission mode: {permission_mode}\n\
         - /proc: {proc_line}\n"
    )
}
