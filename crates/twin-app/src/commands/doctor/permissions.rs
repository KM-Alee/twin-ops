use std::fs;

use crate::model::{DoctorPermissions, PermissionMode};

pub fn check() -> DoctorPermissions {
    let base_mode = detect_permission_mode();
    let proc_accessible = fs::read_dir("/proc").is_ok();

    if !proc_accessible {
        return DoctorPermissions {
            mode: base_mode,
            proc_accessible: false,
            readable_process_count: None,
            restricted_process_count: None,
        };
    }

    let (readable, restricted) = count_process_access();
    let mode = if matches!(base_mode, PermissionMode::Privileged) {
        PermissionMode::Privileged
    } else if restricted > 0 && readable > 0 {
        PermissionMode::Partial
    } else {
        PermissionMode::Unprivileged
    };

    DoctorPermissions {
        mode,
        proc_accessible: true,
        readable_process_count: Some(readable),
        restricted_process_count: Some(restricted),
    }
}

fn detect_permission_mode() -> PermissionMode {
    if let Ok(status) = fs::read_to_string("/proc/self/status") {
        let effective_uid = status.lines().find_map(|line| {
            line.strip_prefix("Uid:")?
                .split_whitespace()
                .nth(1)?
                .parse::<u32>()
                .ok()
        });
        if effective_uid == Some(0) {
            return PermissionMode::Privileged;
        }
    }
    PermissionMode::Unprivileged
}

fn count_process_access() -> (usize, usize) {
    let Ok(entries) = fs::read_dir("/proc") else {
        return (0, 0);
    };

    let mut readable = 0usize;
    let mut restricted = 0usize;

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        if !name.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if fs::metadata(entry.path()).is_ok() {
            readable += 1;
        } else {
            restricted += 1;
        }
    }

    (readable, restricted)
}
