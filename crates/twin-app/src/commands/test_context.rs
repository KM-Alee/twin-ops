use twin_core::SnapshotId;
use twin_store::{EdgeRow, NodeRow, Store};
use twin_test::{
    BaselineComparison, CheckKind, CheckStatus, EvaluationContext, TestDocument, TestRunReport,
    UnknownFact,
};

use crate::commands::test_cmd::TestRunOptions;
use crate::error::{AppError, ScanError, TemporalError};

type GraphRows = (Vec<NodeRow>, Vec<EdgeRow>, i64);

pub(crate) fn validate_snapshots(store: &Store, options: &TestRunOptions) -> Result<(), AppError> {
    for name in [options.snapshot.as_deref(), options.baseline.as_deref()]
        .into_iter()
        .flatten()
    {
        let id = SnapshotId::new(name)
            .map_err(|source| TemporalError::InvalidSnapshotName { source })?;
        if !store
            .snapshot_exists(id.as_str())
            .map_err(ScanError::Store)?
        {
            return Err(TemporalError::SnapshotNotFound { name: name.into() }.into());
        }
    }
    Ok(())
}

pub(crate) fn graph_rows(
    store: &Store,
    snapshot: Option<&str>,
    now: i64,
) -> Result<GraphRows, AppError> {
    if let Some(name) = snapshot {
        let name = SnapshotId::new(name)
            .map_err(|source| TemporalError::InvalidSnapshotName { source })?;
        let row = store
            .list_snapshots()
            .map_err(ScanError::Store)?
            .into_iter()
            .find(|row| row.name == name.as_str())
            .ok_or_else(|| TemporalError::SnapshotNotFound {
                name: name.to_string(),
            })?;
        Ok((
            store
                .load_snapshot_nodes(name.as_str())
                .map_err(ScanError::Store)?,
            store
                .load_snapshot_edges(name.as_str())
                .map_err(ScanError::Store)?,
            row.created_at_ns,
        ))
    } else {
        Ok((
            store.list_nodes().map_err(ScanError::Store)?,
            store.list_current_edges().map_err(ScanError::Store)?,
            now,
        ))
    }
}

pub(crate) fn is_old(observed: i64, at: i64, max_age: Option<u64>) -> bool {
    observed <= 0
        || observed > at
        || max_age.is_some_and(|seconds| {
            observed <= 0
                || at.saturating_sub(observed)
                    > i64::try_from(seconds)
                        .unwrap_or(i64::MAX)
                        .saturating_mul(1_000_000_000)
        })
}

pub(crate) fn unknowns(
    store: &Store,
    snapshot: Option<&str>,
    at: i64,
    max_age: Option<u64>,
    mut unknowns: Vec<UnknownFact>,
    document: &TestDocument,
) -> Result<Vec<UnknownFact>, AppError> {
    if snapshot.is_some() {
        unknowns = super::coverage::load_at(store, at)?
            .unknown_lines()
            .into_iter()
            .map(|line| UnknownFact {
                source: super::test_context::coverage_source(line.kind, &line.detail),
                kind: line.kind.into(),
                detail: line.detail,
            })
            .collect();
    }
    match super::coverage::latest_run_at(store, twin_collectors::COLLECTOR_NAME, at)
        .map_err(ScanError::Store)?
    {
        None => unknowns.push(UnknownFact {
            source: Some(twin_test::FactSource::Host),
            kind: "unavailable_collector".into(),
            detail: "process collector has never completed; runtime absence is unknown".into(),
        }),
        Some(run) if run.status != "success" || is_old(run.ended_at_ns, at, max_age) => unknowns
            .push(UnknownFact {
                source: Some(twin_test::FactSource::Host),
                kind: "stale_evidence".into(),
                detail: "process scan failed or exceeds maximum evidence age".into(),
            }),
        Some(_) => {}
    }
    for (collector, source) in required_sources(document) {
        if collector == twin_collectors::COLLECTOR_NAME {
            continue;
        }
        let run = super::coverage::latest_run_at(store, collector, at).map_err(ScanError::Store)?;
        if run
            .as_ref()
            .is_none_or(|run| run.status != "success" || is_old(run.ended_at_ns, at, max_age))
        {
            unknowns.push(UnknownFact {
                source: Some(source),
                kind: "unavailable_collector".into(),
                detail: format!(
                    "{collector} coverage is missing, incomplete or exceeds maximum evidence age"
                ),
            });
        }
    }
    Ok(unknowns)
}

pub(crate) fn context(
    store: &Store,
    options: &TestRunOptions,
    now: i64,
    document: &TestDocument,
) -> Result<EvaluationContext, AppError> {
    let graph_observed_at_ns = if let Some(name) = &options.snapshot {
        store
            .list_snapshots()
            .map_err(ScanError::Store)?
            .into_iter()
            .find(|row| &row.name == name)
            .map(|row| row.created_at_ns)
    } else {
        store
            .latest_collector_run(twin_collectors::COLLECTOR_NAME)
            .map_err(ScanError::Store)?
            .map(|run| run.ended_at_ns)
    };
    let mut live_sources = Vec::new();
    if options.snapshot.is_none() {
        if document
            .checks
            .iter()
            .any(|check| matches!(check.kind, CheckKind::Disk { .. }))
        {
            live_sources.push("disk usage".into());
        }
        if document
            .checks
            .iter()
            .any(|check| matches!(check.kind, CheckKind::Outbound { .. }))
        {
            live_sources.push("DNS allowlist resolution".into());
        }
    }
    Ok(EvaluationContext {
        evaluated_at_ns: now,
        graph_observed_at_ns,
        snapshot: options.snapshot.clone(),
        max_age_seconds: options.max_age_seconds,
        live_sources,
    })
}

pub(crate) fn compare(
    snapshot: &str,
    before: &TestRunReport,
    current: &TestRunReport,
) -> BaselineComparison {
    let mut comparison = BaselineComparison {
        snapshot: snapshot.into(),
        newly_failing: Vec::new(),
        newly_uncertain: Vec::new(),
        recovered: Vec::new(),
        checks: before.checks.clone(),
    };
    for after in &current.checks {
        let Some(before) = before.checks.iter().find(|check| check.name == after.name) else {
            continue;
        };
        if after.status == CheckStatus::Fail && before.status != CheckStatus::Fail {
            comparison.newly_failing.push(after.name.clone());
        }
        if after.status == CheckStatus::Warn && before.status == CheckStatus::Pass {
            comparison.newly_uncertain.push(after.name.clone());
        }
        if after.status == CheckStatus::Pass && before.status != CheckStatus::Pass {
            comparison.recovered.push(after.name.clone());
        }
    }
    comparison
}

pub(crate) fn coverage_source(kind: &str, detail: &str) -> Option<twin_test::FactSource> {
    use twin_test::FactSource;
    if kind == "permission_gap" || kind == "unmapped_sockets" {
        return Some(FactSource::Host);
    }
    if kind == "unavailable_collector" {
        if detail.starts_with("containerd ") {
            return Some(FactSource::Containerd);
        }
        if detail.starts_with("proc_net_tcp ") {
            return Some(FactSource::Tcp);
        }
        if detail.starts_with("proc_net_udp ") {
            return Some(FactSource::Udp);
        }
        if detail.starts_with("proc_net_unix ") {
            return Some(FactSource::Unix);
        }
        if detail.starts_with("systemd_") {
            return Some(FactSource::Systemd);
        }
    }
    None
}

fn required_sources(
    document: &TestDocument,
) -> std::collections::BTreeMap<&'static str, twin_test::FactSource> {
    let mut sources = std::collections::BTreeMap::new();
    for check in &document.checks {
        let ids = match &check.kind {
            CheckKind::Node(id) | CheckKind::Port(id) | CheckKind::Service(id) => vec![id],
            CheckKind::ServiceState { service, .. } => vec![service],
            CheckKind::Listener { port, owned_by } => vec![port, owned_by],
            CheckKind::Dependency { from, to } => vec![from, to],
            CheckKind::ContainerHealth { container, .. } => vec![container],
            CheckKind::K8sReady {
                deployment,
                routed_service,
                ..
            } => std::iter::once(deployment)
                .chain(routed_service.iter())
                .collect(),
            CheckKind::Outbound { from, .. } => from.iter().collect(),
            _ => Vec::new(),
        };
        for id in ids {
            use twin_core::NodeKind;
            use twin_test::FactSource;
            let (collector, source) = match id.kind() {
                Some(NodeKind::Container | NodeKind::Image) => {
                    if id.as_str().contains(":containerd:") {
                        ("containerd", FactSource::Containerd)
                    } else {
                        ("docker", FactSource::Docker)
                    }
                }
                Some(
                    NodeKind::K8sNamespace
                    | NodeKind::K8sPod
                    | NodeKind::K8sDeployment
                    | NodeKind::K8sReplicaSet
                    | NodeKind::K8sService
                    | NodeKind::K8sEndpoint
                    | NodeKind::K8sConfigMap
                    | NodeKind::K8sSecretRef
                    | NodeKind::K8sPvc
                    | NodeKind::K8sIngress
                    | NodeKind::K8sEvent,
                ) => ("k8s", FactSource::Kubernetes),
                Some(NodeKind::Service) => ("systemd_unit", FactSource::Systemd),
                _ => (twin_collectors::COLLECTOR_NAME, FactSource::Host),
            };
            sources.insert(collector, source);
        }
    }
    sources
}

pub(crate) fn mark_historical_unavailable(document: &TestDocument, report: &mut TestRunReport) {
    for result in &mut report.checks {
        if document.checks.iter().any(|check| {
            check.name == result.name
                && matches!(
                    check.kind,
                    CheckKind::Disk { .. } | CheckKind::Emulate { .. }
                )
        }) {
            result.status = if document.fail_on_unknown {
                CheckStatus::Fail
            } else {
                CheckStatus::Warn
            };
            result.detail = Some("Unknown: this check requires live measurements or a current-state emulation; graph snapshots do not retain those measurements".into());
            result.evidence = None;
        }
    }
    report.passed = 0;
    report.warned = 0;
    report.failed = 0;
    for check in &report.checks {
        match check.status {
            CheckStatus::Pass => report.passed += 1,
            CheckStatus::Warn => report.warned += 1,
            CheckStatus::Fail => report.failed += 1,
        }
    }
}
