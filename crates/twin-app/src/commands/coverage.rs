use std::path::{Path, PathBuf};

use twin_collectors::{ProcessWarningKind, COLLECTOR_NAME, RUNTIME_COLLECTOR_NAME};
use twin_core::EvidenceRecency;
use twin_store::Store;

use crate::error::ScanError;
use crate::model::{CoverageReport, CoverageUnknown, ImpactUnknown};

pub(crate) struct AvailabilityProbe {
    pub ebpf_available: bool,
    pub docker_available: bool,
    pub kubernetes_available: bool,
}

impl AvailabilityProbe {
    pub(crate) fn host() -> Self {
        let facts = twin_ebpf::host_facts();
        let report = twin_ebpf::assess(&facts);
        Self {
            ebpf_available: report.kernel.ok && report.btf.ok && report.capabilities.ok,
            docker_available: Path::new("/var/run/docker.sock").exists(),
            kubernetes_available: kubernetes_available(),
        }
    }
}

fn kubernetes_available() -> bool {
    if Path::new("/var/run/secrets/kubernetes.io/serviceaccount/token").exists() {
        return true;
    }
    kubeconfig_path().is_some_and(|path| path.exists())
}

fn kubeconfig_path() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("KUBECONFIG") {
        return Some(PathBuf::from(path));
    }
    std::env::var_os("HOME").map(|home| Path::new(&home).join(".kube/config"))
}

pub(crate) fn from_scan_counts(
    readable_processes: usize,
    warnings: &[twin_collectors::ProcessWarning],
    dbus_available: bool,
    unmapped_active_socket_count: usize,
    probe: &AvailabilityProbe,
) -> CoverageReport {
    let mut permission_denied = 0usize;
    let mut fd_permission_denied = 0usize;
    let mut cgroup_permission_denied = 0usize;
    let mut socket_unmapped = 0usize;
    let mut active_socket_unmapped = 0usize;
    let mut unix_socket_unmapped = 0usize;
    let mut unix_connection_unmapped = 0usize;
    let mut tcp_table_missing = false;
    let mut unix_table_missing = false;
    for warning in warnings {
        match warning.kind() {
            ProcessWarningKind::PermissionDenied => permission_denied += 1,
            ProcessWarningKind::FdPermissionDenied => fd_permission_denied += 1,
            ProcessWarningKind::CgroupPermissionDenied => cgroup_permission_denied += 1,
            ProcessWarningKind::SocketUnmapped => socket_unmapped += 1,
            ProcessWarningKind::ActiveSocketUnmapped => active_socket_unmapped += 1,
            ProcessWarningKind::UnixSocketUnmapped => unix_socket_unmapped += 1,
            ProcessWarningKind::UnixConnectionUnmapped => unix_connection_unmapped += 1,
            ProcessWarningKind::TcpTableMissing => tcp_table_missing = true,
            ProcessWarningKind::UnixTableMissing => unix_table_missing = true,
            _ => {}
        }
    }
    let mut unavailable = Vec::new();
    if tcp_table_missing {
        unavailable.push("proc_net_tcp".to_string());
    }
    if unix_table_missing {
        unavailable.push("proc_net_unix".to_string());
    }
    if !dbus_available {
        unavailable.push("systemd_dbus".to_string());
    }
    CoverageReport {
        readable_processes,
        restricted_processes: permission_denied + fd_permission_denied + cgroup_permission_denied,
        unmapped_sockets: socket_unmapped
            + active_socket_unmapped.max(unmapped_active_socket_count)
            + unix_socket_unmapped
            + unix_connection_unmapped,
        unavailable_collectors: unavailable,
        ebpf_available: probe.ebpf_available,
        docker_available: probe.docker_available,
        kubernetes_available: probe.kubernetes_available,
    }
}

pub(crate) fn load(store: &Store) -> Result<CoverageReport, ScanError> {
    let probe = AvailabilityProbe::host();
    let mut report = CoverageReport {
        ebpf_available: probe.ebpf_available,
        docker_available: probe.docker_available,
        kubernetes_available: probe.kubernetes_available,
        ..CoverageReport::default()
    };
    if let Some(run) = store.latest_collector_run(COLLECTOR_NAME)? {
        if let Ok(meta) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&run.metadata_json)
        {
            report.readable_processes = meta_usize(&meta, "readable_processes");
            report.restricted_processes = meta_usize(&meta, "permission_denied")
                + meta_usize(&meta, "fd_permission_denied")
                + meta_usize(&meta, "cgroup_permission_denied");
            report.unmapped_sockets = meta_usize(&meta, "socket_unmapped")
                + meta_usize(&meta, "active_socket_unmapped")
                + meta_usize(&meta, "unix_socket_unmapped")
                + meta_usize(&meta, "unix_connection_unmapped");
            if meta_usize(&meta, "tcp_table_missing") > 0 {
                report
                    .unavailable_collectors
                    .push("proc_net_tcp".to_string());
            }
            if meta_usize(&meta, "unix_table_missing") > 0 {
                report
                    .unavailable_collectors
                    .push("proc_net_unix".to_string());
            }
        }
    }
    if let Some(run) = store.latest_collector_run(RUNTIME_COLLECTOR_NAME)? {
        if let Ok(meta) =
            serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&run.metadata_json)
        {
            if meta.get("dbus_available").and_then(|value| value.as_bool()) == Some(false) {
                report
                    .unavailable_collectors
                    .push("systemd_dbus".to_string());
            }
        }
    }
    report.unavailable_collectors.sort();
    report.unavailable_collectors.dedup();
    Ok(report)
}

pub(crate) fn reference_ns(store: &Store) -> Result<Option<i64>, ScanError> {
    Ok(store
        .latest_collector_run(COLLECTOR_NAME)?
        .map(|run| run.ended_at_ns))
}

pub(crate) fn unknowns(coverage: &CoverageReport) -> Vec<ImpactUnknown> {
    coverage
        .unknown_lines()
        .into_iter()
        .map(impact_unknown)
        .collect()
}

fn impact_unknown(line: CoverageUnknown) -> ImpactUnknown {
    ImpactUnknown {
        kind: line.kind.to_string(),
        detail: line.detail,
        source: Some("coverage".to_string()),
        weakens_evidence: false,
    }
}

pub(crate) fn recency_between(
    observed_ns: Option<i64>,
    reference_ns: Option<i64>,
) -> EvidenceRecency {
    match (observed_ns, reference_ns) {
        (Some(observed), Some(reference)) => EvidenceRecency::from_timestamps(observed, reference),
        _ => EvidenceRecency::Unknown,
    }
}

fn meta_usize(meta: &serde_json::Map<String, serde_json::Value>, key: &str) -> usize {
    meta.get(key).and_then(|value| value.as_u64()).unwrap_or(0) as usize
}
