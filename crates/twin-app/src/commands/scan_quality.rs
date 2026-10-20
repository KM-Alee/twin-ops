use std::collections::HashMap;

use twin_collectors::{ProcessWarningKind, COLLECTOR_NAME, RUNTIME_COLLECTOR_NAME};
use twin_store::Store;

use crate::error::ScanError;
use crate::model::{ScanQuality, ScanQualityAssessment};

const FD_DEGRADED_THRESHOLD: usize = 50;
const SOCKET_UNMAPPED_DEGRADED_THRESHOLD: usize = 3;

pub fn assess_scan_quality(store: &Store) -> Result<ScanQualityAssessment, ScanError> {
    let mut reasons = Vec::new();
    let mut fd_hidden = 0usize;
    let mut socket_unmapped = 0usize;
    let mut active_unmapped = 0usize;
    let mut unix_socket_unmapped = 0usize;
    let mut unix_connection_unmapped = 0usize;

    let mut samples_total = 1u32;
    let mut tcp_connection_count = 0usize;

    if let Some(run) = store
        .latest_collector_run(COLLECTOR_NAME)
        .map_err(ScanError::Store)?
    {
        if let Ok(meta) =
            serde_json::from_str::<HashMap<String, serde_json::Value>>(&run.metadata_json)
        {
            fd_hidden = meta_usize(&meta, "fd_permission_denied");
            socket_unmapped = meta_usize(&meta, "socket_unmapped");
            active_unmapped = meta_usize(&meta, "active_socket_unmapped");
            unix_socket_unmapped = meta_usize(&meta, "unix_socket_unmapped");
            unix_connection_unmapped = meta_usize(&meta, "unix_connection_unmapped");
            samples_total = meta
                .get("samples_total")
                .and_then(|v| v.as_u64())
                .unwrap_or(1) as u32;
            tcp_connection_count = meta_usize(&meta, "tcp_connection_count")
                + meta_usize(&meta, "udp_connection_count");
        } else if run.warning_count > 0 {
            reasons.push(format!(
                "{} collector warnings in latest process scan",
                run.warning_count
            ));
        }
    }

    if fd_hidden > 0 {
        reasons.push(format!("{fd_hidden} fd directories hidden by permissions"));
    }
    if socket_unmapped > 0 {
        reasons.push(format!(
            "{socket_unmapped} listener sockets could not be mapped to a process"
        ));
    }
    if active_unmapped > 0 {
        reasons.push(format!(
            "{active_unmapped} active network sockets could not be mapped to a process"
        ));
    }
    if unix_socket_unmapped > 0 {
        reasons.push(format!(
            "{unix_socket_unmapped} unix listener sockets could not be mapped to a process"
        ));
    }
    if unix_connection_unmapped > 0 {
        reasons.push(format!(
            "{unix_connection_unmapped} unix client sockets could not be mapped to a process"
        ));
    }

    if let Some(run) = store
        .latest_collector_run(RUNTIME_COLLECTOR_NAME)
        .map_err(ScanError::Store)?
    {
        let runtime_ran = run.observation_count > 0 || run.warning_count > 0;
        if runtime_ran {
            if let Ok(meta) =
                serde_json::from_str::<HashMap<String, serde_json::Value>>(&run.metadata_json)
            {
                if meta.get("dbus_available").and_then(|v| v.as_bool()) == Some(false) {
                    reasons.push(
                        "systemd D-Bus unavailable at last scan — runtime Requires/Wants from bus missing"
                            .to_string(),
                    );
                }
            }
        }
    }

    let unmapped_total =
        socket_unmapped + active_unmapped + unix_socket_unmapped + unix_connection_unmapped;
    let coverage = super::coverage::load(store)?;
    let unavailable = !coverage.unavailable_collectors.is_empty();
    for line in coverage.unknown_lines() {
        if !reasons.contains(&line.detail) {
            reasons.push(line.detail);
        }
    }
    let never_scanned = store.latest_collector_run(COLLECTOR_NAME)?.is_none();
    if never_scanned {
        reasons.push("process collector has never completed".into());
    }
    let quality = if unavailable
        || never_scanned
        || fd_hidden >= FD_DEGRADED_THRESHOLD
        || unmapped_total >= SOCKET_UNMAPPED_DEGRADED_THRESHOLD
    {
        ScanQuality::Degraded
    } else if coverage.restricted_processes > 0 || fd_hidden > 0 || unmapped_total > 0 {
        ScanQuality::Partial
    } else if reasons.is_empty() {
        ScanQuality::Good
    } else {
        ScanQuality::Partial
    };

    let impact_reliable = !matches!(quality, ScanQuality::Degraded);
    let ephemeral_capture_recommended =
        samples_total == 1 && (tcp_connection_count > 0 || active_unmapped > 0);

    Ok(ScanQualityAssessment {
        quality,
        reasons,
        impact_reliable,
        ephemeral_capture_recommended,
    })
}

fn meta_usize(meta: &HashMap<String, serde_json::Value>, key: &str) -> usize {
    meta.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as usize
}

pub fn scan_health_note(assessment: &ScanQualityAssessment) -> Option<String> {
    match assessment.quality {
        ScanQuality::Good => None,
        ScanQuality::Partial => Some(
            "scan quality partial — some runtime dependency detection may be incomplete"
                .to_string(),
        ),
        ScanQuality::Degraded => Some(
            "scan quality degraded — runtime socket dependency detection may be incomplete; declared systemd dependencies still available"
                .to_string(),
        ),
    }
}

pub fn warning_metadata_from_process_warnings(
    warnings: &[twin_collectors::ProcessWarning],
) -> String {
    process_collector_metadata(ProcessCollectorMetadataInput {
        warnings,
        sample_index: 0,
        samples_total: 1,
        tcp_connection_count: 0,
        udp_connection_count: 0,
        unmapped_active_socket_count: 0,
        readable_processes: 0,
    })
}

/// Distinct warning groups kept per collector run; host-wide warnings repeat per process.
const WARNING_GROUP_CAP: usize = 64;

/// One entry per (kind, detail pattern) with its first detail, first path and occurrence count, so every run's
/// stored metadata stays bounded however many processes repeat the same warning.
fn grouped_warning_details(
    warnings: &[twin_collectors::ProcessWarning],
) -> (Vec<serde_json::Value>, usize) {
    let mut groups: Vec<(&str, String, String, String, usize)> = Vec::new();
    for warning in warnings {
        let kind = warning.kind().detail_key();
        let pattern = warning_pattern(warning.detail());
        match groups
            .iter_mut()
            .find(|(known_kind, known, _, _, _)| *known_kind == kind && *known == pattern)
        {
            Some(group) => group.4 += 1,
            None => groups.push((
                kind,
                pattern,
                warning.detail().to_string(),
                warning.path().display().to_string(),
                1,
            )),
        }
    }
    let omitted = groups.len().saturating_sub(WARNING_GROUP_CAP);
    groups.truncate(WARNING_GROUP_CAP);
    let details = groups
        .into_iter()
        .map(|(kind, _, detail, path, count)| {
            serde_json::json!({"kind": kind, "path": path, "detail": detail, "count": count})
        })
        .collect();
    (details, omitted)
}

pub(crate) struct ProcessCollectorMetadataInput<'a> {
    pub warnings: &'a [twin_collectors::ProcessWarning],
    pub sample_index: u32,
    pub samples_total: u32,
    pub tcp_connection_count: usize,
    pub udp_connection_count: usize,
    pub unmapped_active_socket_count: usize,
    pub readable_processes: usize,
}

pub(crate) fn process_collector_metadata(input: ProcessCollectorMetadataInput<'_>) -> String {
    let mut permission_denied = 0usize;
    let mut fd_permission_denied = 0usize;
    let mut cgroup_permission_denied = 0usize;
    let mut socket_unmapped = 0usize;
    let mut active_socket_unmapped = 0usize;
    let mut unix_socket_unmapped = 0usize;
    let mut unix_connection_unmapped = 0usize;
    let mut tcp_table_missing = 0usize;
    let mut unix_table_missing = 0usize;
    let mut udp_table_unavailable = 0usize;
    for w in input.warnings {
        match w.kind() {
            ProcessWarningKind::PermissionDenied => permission_denied += 1,
            ProcessWarningKind::FdPermissionDenied => fd_permission_denied += 1,
            ProcessWarningKind::CgroupPermissionDenied => cgroup_permission_denied += 1,
            ProcessWarningKind::SocketUnmapped => socket_unmapped += 1,
            ProcessWarningKind::ActiveSocketUnmapped => active_socket_unmapped += 1,
            ProcessWarningKind::UnixSocketUnmapped => unix_socket_unmapped += 1,
            ProcessWarningKind::UnixConnectionUnmapped => unix_connection_unmapped += 1,
            ProcessWarningKind::TcpTableMissing => tcp_table_missing += 1,
            ProcessWarningKind::UnixTableMissing => unix_table_missing += 1,
            ProcessWarningKind::UdpTableMissing | ProcessWarningKind::UdpTableMalformed => {
                udp_table_unavailable += 1
            }
            _ => {}
        }
    }
    let (warning_details, warning_details_omitted) = grouped_warning_details(input.warnings);
    serde_json::json!({
        "warning_details": warning_details,
        "warning_details_omitted": warning_details_omitted,
        "permission_denied": permission_denied,
        "fd_permission_denied": fd_permission_denied,
        "cgroup_permission_denied": cgroup_permission_denied,
        "socket_unmapped": socket_unmapped,
        "active_socket_unmapped": active_socket_unmapped.max(input.unmapped_active_socket_count),
        "unix_socket_unmapped": unix_socket_unmapped,
        "unix_connection_unmapped": unix_connection_unmapped,
        "tcp_table_missing": tcp_table_missing,
        "unix_table_missing": unix_table_missing,
        "udp_table_unavailable": udp_table_unavailable,
        "readable_processes": input.readable_processes,
        "sample_index": input.sample_index,
        "samples_total": input.samples_total,
        "tcp_connection_count": input.tcp_connection_count,
        "udp_connection_count": input.udp_connection_count,
    })
    .to_string()
}

/// Masks numbers, paths and IPv6 literals so per-pid/per-socket warnings share one
/// pattern.
pub(crate) fn warning_pattern(detail: &str) -> String {
    let mut pattern = String::with_capacity(detail.len());
    let mut in_brackets = false;
    let mut in_path = false;
    for ch in detail.chars() {
        if in_path {
            if ch.is_whitespace() {
                in_path = false;
                pattern.push(ch);
            }
            continue;
        }
        match ch {
            '/' | '@' if pattern.is_empty() || pattern.ends_with(' ') => {
                in_path = true;
                pattern.push_str("/#");
            }
            '[' => {
                in_brackets = true;
                pattern.push_str("[#");
            }
            ']' => {
                in_brackets = false;
                pattern.push(']');
            }
            _ if in_brackets => {}
            '0'..='9' => {
                if !pattern.ends_with('#') {
                    pattern.push('#');
                }
            }
            other => pattern.push(other),
        }
    }
    pattern
}
