use twin_app::{DoctorResult, PermissionMode};

use crate::output::format::{status_tag, Lines, Status};

pub fn render(result: &DoctorResult) -> String {
    let mut out = Lines::new();
    out.title("twin doctor");

    let overall = if result.core.config_found && result.database.initialized {
        Status::Ok
    } else if result.database.initialized || result.core.config_found {
        Status::Warn
    } else {
        Status::Bad
    };
    out.status_row(
        overall,
        "health",
        if result.database.initialized {
            "ready to scan"
        } else {
            "run twin init first"
        },
    );
    out.blank();
    out.section("checks");

    out.tree_leaf(false, "cli", if result.core.cli_ok { "ok" } else { "fail" });

    let config_path = result
        .core
        .config_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let (config_status, config_detail) = if result.core.config_found {
        (Status::Ok, config_path)
    } else {
        (Status::Bad, "not found".to_string())
    };
    out.tree_leaf(
        false,
        "config",
        &format!("{}  {}", status_tag(config_status), config_detail),
    );

    let db_path = result
        .database
        .db_path
        .as_ref()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let db_detail = if result.database.initialized {
        let version = result
            .database
            .schema_version
            .map(|v| format!(", schema v{v}"))
            .unwrap_or_default();
        let wal = match result.database.wal_mode {
            Some(true) => ", wal",
            Some(false) => ", not wal",
            None => "",
        };
        format!("initialized{version}{wal}  {db_path}")
    } else {
        format!("not initialized  {db_path}")
    };
    out.tree_leaf(
        false,
        "database",
        &format!(
            "{}  {}",
            status_tag(if result.database.initialized {
                Status::Ok
            } else {
                Status::Bad
            }),
            db_detail
        ),
    );

    let perm = match result.permissions.mode {
        PermissionMode::Privileged => "privileged",
        PermissionMode::Unprivileged => "unprivileged",
        PermissionMode::Partial => "partial",
    };
    out.tree_leaf(false, "permissions", perm);

    let proc_detail = if result.permissions.proc_accessible {
        match (
            result.permissions.readable_process_count,
            result.permissions.restricted_process_count,
        ) {
            (Some(readable), Some(restricted)) => {
                format!("{readable} readable, {restricted} restricted")
            }
            _ => "accessible".to_string(),
        }
    } else {
        "not accessible".to_string()
    };
    out.tree_leaf(
        true,
        "/proc",
        &format!(
            "{}  {}",
            status_tag(if result.permissions.proc_accessible {
                Status::Ok
            } else {
                Status::Bad
            }),
            proc_detail
        ),
    );

    if matches!(result.permissions.mode, PermissionMode::Partial) {
        out.blank();
        out.section("notes");
        out.tree_leaf(
            true,
            "listeners",
            "partial /proc access hides other users' fd dirs; run `sudo twin scan` to map all TCP listeners",
        );
    }

    if let Some(quality) = &result.scan_quality {
        out.blank();
        let status = match quality.quality {
            twin_app::ScanQuality::Good => Status::Ok,
            twin_app::ScanQuality::Partial => Status::Warn,
            twin_app::ScanQuality::Degraded => Status::Bad,
        };
        out.status_row(
            status,
            "scan quality",
            quality.quality.as_str().to_uppercase().as_str(),
        );
        if !quality.reasons.is_empty() {
            out.section("scan quality reasons");
            for (i, reason) in quality.reasons.iter().enumerate() {
                let is_last = i + 1 == quality.reasons.len();
                out.tree_leaf(is_last, "reason", reason);
            }
        }
        if !quality.impact_reliable {
            out.blank();
            out.tree_leaf(
                true,
                "impact",
                "runtime-only impact conclusions may be incomplete; declared systemd deps still available",
            );
        }
        if quality.ephemeral_capture_recommended {
            out.blank();
            out.tree_leaf(
                true,
                "ephemeral capture",
                "re-run `twin scan --samples 5 --interval 2` to merge short-lived connections across samples",
            );
        }
    } else if let Some(err) = &result.scan_quality_error {
        out.blank();
        out.tree_leaf(true, "scan quality", &format!("unavailable ({err})"));
    }

    out.into_string()
}
