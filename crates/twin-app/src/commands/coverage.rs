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
            docker_available: false,
            kubernetes_available: false,
        }
    }
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
    let mut udp_table_unavailable = false;
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
            ProcessWarningKind::UdpTableMissing | ProcessWarningKind::UdpTableMalformed => {
                udp_table_unavailable = true
            }
            _ => {}
        }
    }
    let mut unavailable = Vec::new();
    if tcp_table_missing {
        unavailable.push("proc_net_tcp".to_string());
    }
    if udp_table_unavailable {
        unavailable.push("proc_net_udp".to_string());
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
        containerd_available: false,
        kubernetes_available: probe.kubernetes_available,
    }
}

pub(crate) fn load(store: &Store) -> Result<CoverageReport, ScanError> {
    load_at(store, i64::MAX)
}

pub(crate) fn load_at(store: &Store, at_ns: i64) -> Result<CoverageReport, ScanError> {
    let probe = AvailabilityProbe::host();
    let mut report = CoverageReport {
        ebpf_available: probe.ebpf_available,
        docker_available: probe.docker_available,
        kubernetes_available: probe.kubernetes_available,
        ..CoverageReport::default()
    };
    if let Some(run) = latest_run_at(store, COLLECTOR_NAME, at_ns)? {
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
            if meta_usize(&meta, "udp_table_unavailable") > 0 {
                report
                    .unavailable_collectors
                    .push("proc_net_udp".to_string());
            }
            if meta_usize(&meta, "unix_table_missing") > 0 {
                report
                    .unavailable_collectors
                    .push("proc_net_unix".to_string());
            }
        }
    }
    if let Some(run) = latest_run_at(store, RUNTIME_COLLECTOR_NAME, at_ns)? {
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
    if let Some(run) = latest_run_at(store, "k8s", at_ns)? {
        report.kubernetes_available = run.status == "success";
    }
    if let Some(run) = latest_run_at(store, "docker", at_ns)? {
        report.docker_available = run.status == "success";
    }
    if let Some(run) = latest_run_at(store, super::watch::EBPF_COLLECTOR, at_ns)? {
        report.ebpf_available = run.status == "success";
        if !report.ebpf_available {
            report.unavailable_collectors.push("ebpf".into());
        }
    }
    if let Some(run) = latest_run_at(store, "containerd", at_ns)? {
        report.containerd_available = run.status == "success";
        if !report.containerd_available {
            report.unavailable_collectors.push("containerd".into());
        }
    }
    report.unavailable_collectors.sort();
    report.unavailable_collectors.dedup();
    Ok(report)
}

pub(crate) fn reference_ns(_store: &Store) -> Result<Option<i64>, ScanError> {
    Ok(Some(twin_core::TimestampNs::now().as_i64()))
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

pub(crate) fn latest_run_at(
    store: &Store,
    collector: &str,
    at_ns: i64,
) -> Result<Option<twin_store::CollectorRunRow>, twin_store::StoreError> {
    if at_ns == i64::MAX {
        return store.latest_collector_run(collector);
    }
    Ok(store
        .list_collector_runs(Some(collector))?
        .into_iter()
        .filter(|run| run.ended_at_ns <= at_ns)
        .max_by_key(|run| (run.ended_at_ns, run.id)))
}
