use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use twin_core::{EvidenceLabel, NodeId, RiskLevel, TimestampNs};
use twin_store::{EdgeRow, Store, TestRunRow};
use twin_test::{
    evaluate, parse_file, write_starter, CheckKind, DiskFact, EdgeFact, EmulateAction,
    EmulationFact, EndpointFact, GraphFacts, NodeFact, ServiceFact, TestDocument, TestRunReport,
    UnknownFact,
};
use uuid::Uuid;

use crate::error::{AppError, ScanError, TestCmdError};
use crate::paths::TwinLayout;

pub struct TestInitRequest {
    pub path: PathBuf,
    pub force: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestInitOutcome {
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
pub struct TestRunSummary {
    pub name: String,
    pub path: String,
    pub started_at_ns: i64,
    pub passed: i64,
    pub warned: i64,
    pub failed: i64,
}

pub fn list_test_runs_in(layout: &TwinLayout) -> Result<Vec<TestRunSummary>, AppError> {
    let db = layout.db_file();
    if !db.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let store = Store::open(&db).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let mut runs = Vec::new();
    for row in store.list_test_runs().map_err(ScanError::Store)? {
        runs.push(TestRunSummary {
            name: row.name,
            path: row.file_path,
            started_at_ns: row.started_at_ns,
            passed: row.passed,
            warned: row.warned,
            failed: row.failed,
        });
    }
    Ok(runs)
}

pub fn init_file(request: &TestInitRequest) -> Result<TestInitOutcome, AppError> {
    write_starter(&request.path, request.force).map_err(TestCmdError::Test)?;
    Ok(TestInitOutcome {
        path: request.path.clone(),
    })
}

pub fn lint_file(path: &Path) -> Result<TestDocument, AppError> {
    Ok(parse_file(path).map_err(TestCmdError::Test)?)
}

#[derive(Debug, Clone, Default)]
pub struct TestRunOptions {
    pub refresh: bool,
    pub max_age_seconds: Option<u64>,
    pub snapshot: Option<String>,
    pub baseline: Option<String>,
}

pub fn run_file(layout: &TwinLayout, path: &Path) -> Result<TestRunReport, AppError> {
    run_file_with_options(layout, path, &TestRunOptions::default())
}

pub fn run_file_with_options(
    layout: &TwinLayout,
    path: &Path,
    options: &TestRunOptions,
) -> Result<TestRunReport, AppError> {
    run_with_dns_options(
        layout,
        path,
        &twin_collectors::dns::SystemDnsReader,
        options,
    )
}

pub fn run_file_with_dns(
    layout: &TwinLayout,
    path: &Path,
    dns: &impl twin_collectors::dns::DnsReader,
) -> Result<TestRunReport, AppError> {
    run_with_dns_options(layout, path, dns, &TestRunOptions::default())
}

fn run_with_dns_options(
    layout: &TwinLayout,
    path: &Path,
    dns: &impl twin_collectors::dns::DnsReader,
    options: &TestRunOptions,
) -> Result<TestRunReport, AppError> {
    let original_document = lint_file(path)?;
    let mut document = original_document.clone();
    if options.refresh && options.snapshot.is_some() {
        return Err(TestCmdError::Encode {
            reason: "--refresh cannot be combined with --snapshot".into(),
        }
        .into());
    }
    let db = layout.db_file();
    if !db.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let mut store = Store::open(&db).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    super::test_context::validate_snapshots(&store, options)?;
    if options.refresh {
        crate::scan_in(layout, crate::ScanRequest::default(), Path::new("/proc"))?;
    }
    let at_ns = TimestampNs::now().as_i64();
    let facts = load_facts(
        layout,
        &store,
        &document,
        options.snapshot.as_deref(),
        at_ns,
        options.max_age_seconds,
    )?;
    let notes = if options.snapshot.is_none() {
        super::test_dns::resolve_allowlists(&mut document, dns)
    } else {
        super::test_dns::historical_allowlists(&mut document)
    };
    let mut report = evaluate(&document, &facts);
    super::test_dns::apply_notes(&mut report, notes);
    if options.snapshot.is_some() {
        super::test_context::mark_historical_unavailable(&document, &mut report);
    }
    report.context = Some(super::test_context::context(
        &store, options, at_ns, &document,
    )?);
    if let Some(baseline) = &options.baseline {
        let mut baseline_document = original_document;
        let notes = super::test_dns::historical_allowlists(&mut baseline_document);
        let facts = load_facts(
            layout,
            &store,
            &baseline_document,
            Some(baseline),
            at_ns,
            options.max_age_seconds,
        )?;
        let mut before = evaluate(&baseline_document, &facts);
        super::test_dns::apply_notes(&mut before, notes);
        super::test_context::mark_historical_unavailable(&baseline_document, &mut before);
        report.baseline = Some(super::test_context::compare(baseline, &before, &report));
    }
    let report_json = serde_json::to_string(&report).map_err(|err| TestCmdError::Encode {
        reason: err.to_string(),
    })?;
    store
        .insert_test_run(&TestRunRow {
            id: Uuid::new_v4().to_string(),
            name: report.name.clone(),
            file_path: path.display().to_string(),
            started_at_ns: at_ns,
            passed: i64::from(report.passed),
            warned: i64::from(report.warned),
            failed: i64::from(report.failed),
            report_json,
        })
        .map_err(ScanError::Store)?;
    Ok(report)
}

fn load_facts(
    layout: &TwinLayout,
    store: &Store,
    document: &TestDocument,
    snapshot: Option<&str>,
    at_ns: i64,
    max_age_seconds: Option<u64>,
) -> Result<GraphFacts, AppError> {
    let (node_rows, edge_rows, as_of) = super::test_context::graph_rows(store, snapshot, at_ns)?;
    let runtime_missing = super::test_health::source_missing(
        store,
        twin_collectors::RUNTIME_COLLECTOR_NAME,
        as_of,
        max_age_seconds,
    )?;
    let mut services = Vec::new();
    let (containers, workloads, routing) =
        super::test_health::facts(store, &node_rows, &edge_rows, as_of, max_age_seconds)?;
    let mut nodes = Vec::new();
    for row in node_rows {
        if row.state == "gone" {
            continue;
        }
        if row.kind == "service" {
            let metadata =
                serde_json::from_str::<serde_json::Value>(&row.metadata_json).unwrap_or_default();
            services.push(ServiceFact {
                id: row.id.clone(),
                active_state: metadata
                    .get("active_state")
                    .and_then(|value| value.as_str())
                    .map(str::to_string),
                stale: runtime_missing
                    || row.state == "stale"
                    || metadata
                        .get("runtime_observed_at_ns")
                        .and_then(|value| value.as_i64())
                        .is_none_or(|observed| {
                            super::test_context::is_old(observed, as_of, max_age_seconds)
                        }),
            });
        }
        nodes.push(NodeFact {
            id: row.id,
            stale: row.state == "stale"
                || super::test_context::is_old(row.last_seen_ns, as_of, max_age_seconds),
        });
    }
    let mut edges = Vec::new();
    let mut endpoints = Vec::new();
    for row in &edge_rows {
        if matches!(
            row.kind.as_str(),
            "depends_on" | "connects_to" | "listens_on"
        ) {
            if row.state == "gone" {
                continue;
            }
            let counted = count_edge(store, row, as_of, snapshot)?;
            if row.kind == "connects_to" {
                if let Some(endpoint) = endpoint_label(&row.to_node_id) {
                    let evidence = endpoint_evidence(&row.from_node_id, counted.ebpf_connects);
                    endpoints.push(EndpointFact {
                        endpoint,
                        evidence,
                        from: row.from_node_id.clone(),
                        stale: row.state == "stale"
                            || super::test_context::is_old(
                                row.last_seen_ns,
                                as_of,
                                max_age_seconds,
                            ),
                    });
                }
            }
            edges.push(EdgeFact {
                from: row.from_node_id.clone(),
                to: row.to_node_id.clone(),
                kind: row.kind.clone(),
                evidence: counted.label,
                supporting_observations: counted.supporting_observations,
                stale: row.state == "stale"
                    || super::test_context::is_old(row.last_seen_ns, as_of, max_age_seconds),
            });
        }
    }
    let direct = endpoints.clone();
    for endpoint in direct {
        let mut frontier = vec![(endpoint.from.clone(), endpoint.stale)];
        let mut seen = BTreeSet::new();
        while let Some((child, stale)) = frontier.pop() {
            if !seen.insert(child.clone()) {
                continue;
            }
            for owner in edge_rows.iter().filter(|edge| {
                edge.to_node_id == child && edge.kind == "owns" && edge.state != "gone"
            }) {
                let mut attributed = endpoint.clone();
                attributed.from = owner.from_node_id.clone();
                attributed.stale |= stale || owner.state == "stale";
                endpoints.push(attributed);
                frontier.push((owner.from_node_id.clone(), stale || owner.state == "stale"));
            }
        }
    }
    Ok(GraphFacts {
        nodes,
        edges,
        services,
        containers,
        workloads,
        routing,
        endpoints,
        disks: if snapshot.is_none() {
            disk_facts(document)
        } else {
            Vec::new()
        },
        unknowns: super::test_context::unknowns(
            store,
            snapshot,
            as_of,
            max_age_seconds,
            unknown_facts(store)?,
            document,
        )?,
        emulations: if snapshot.is_none() {
            emulation_facts(layout, document)
        } else {
            Vec::new()
        },
    })
}

struct CountedEdge {
    label: EvidenceLabel,
    ebpf_connects: u64,
    supporting_observations: usize,
}

fn endpoint_label(port_id: &str) -> Option<String> {
    let id = NodeId::from_str(port_id).ok()?.unscoped_endpoint()?;
    id.as_str()
        .strip_prefix("port:")
        .filter(|endpoint| endpoint.starts_with("tcp:") || endpoint.starts_with("udp:"))
        .map(str::to_string)
}

fn endpoint_evidence(from: &str, ebpf_connects: u64) -> String {
    if ebpf_connects > 0 {
        format!("eBPF observed {from} connecting {ebpf_connects} times")
    } else {
        format!("socket table observed peer for {from}; initiation direction unknown")
    }
}

fn disk_facts(document: &TestDocument) -> Vec<DiskFact> {
    let mut disks = Vec::new();
    for check in &document.checks {
        if let CheckKind::Disk { mount, .. } = &check.kind {
            if let Some(used_percent) = super::disk_usage::mount_used_percent(mount) {
                disks.push(DiskFact {
                    mount: mount.clone(),
                    used_percent,
                });
            }
        }
    }
    disks
}

fn unknown_facts(store: &Store) -> Result<Vec<UnknownFact>, AppError> {
    let report = crate::commands::coverage::load(store)?;
    Ok(report
        .unknown_lines()
        .into_iter()
        .map(|line| UnknownFact {
            source: super::test_context::coverage_source(line.kind, &line.detail),
            kind: line.kind.to_string(),
            detail: line.detail,
        })
        .collect())
}

fn emulation_facts(layout: &TwinLayout, document: &TestDocument) -> Vec<EmulationFact> {
    let mut facts = Vec::new();
    for check in &document.checks {
        let CheckKind::Emulate { action, target, .. } = &check.kind else {
            continue;
        };
        facts.push(run_emulation(layout, *action, target));
    }
    facts
}

fn run_emulation(layout: &TwinLayout, action: EmulateAction, target: &str) -> EmulationFact {
    let request = match action {
        EmulateAction::Restart => {
            let Ok(id) = NodeId::from_str(target) else {
                return failed_emulation(action, target, "invalid service id");
            };
            crate::EmulateRequest {
                action: crate::EmulateActionRequest::Restart {
                    target: Some(id),
                    target_query: None,
                    show_paths: false,
                    max_depth: crate::DEFAULT_MAX_DEPTH,
                },
                show_evidence: true,
                ..crate::EmulateRequest::default()
            }
        }
        EmulateAction::Delete => crate::EmulateRequest {
            action: crate::EmulateActionRequest::DeleteFile {
                path: target.to_string(),
            },
            show_evidence: true,
            ..crate::EmulateRequest::default()
        },
    };
    match crate::emulate_in(layout, request) {
        Ok(result) => {
            let mut evidence = result.evidence_lines;
            evidence.extend(result.evidence_reasons);
            for impact in result
                .transient_impacts
                .iter()
                .chain(result.configured_impacts.iter())
                .chain(result.runtime_impacts.iter())
            {
                evidence.extend(impact.evidence.iter().cloned());
            }
            let evidence_label = if evidence.is_empty() {
                String::new()
            } else {
                result.evidence_strength.label
            };
            let supporting_observations = emulation_support(layout, &result.target);
            EmulationFact {
                action: action.as_str().to_string(),
                target: target.to_string(),
                risk: result.risk.level,
                evidence_label,
                evidence,
                supporting_observations,
            }
        }
        Err(err) => failed_emulation(action, target, &err.to_string()),
    }
}

fn failed_emulation(action: EmulateAction, target: &str, evidence: &str) -> EmulationFact {
    EmulationFact {
        action: action.as_str().to_string(),
        target: target.to_string(),
        risk: RiskLevel::Unknown,
        evidence_label: String::new(),
        evidence: vec![evidence.to_string()],
        supporting_observations: 0,
    }
}

fn count_edge(
    store: &Store,
    row: &EdgeRow,
    at_ns: i64,
    snapshot: Option<&str>,
) -> Result<CountedEdge, AppError> {
    let assessment = if let Some(snapshot) = snapshot {
        let ids = store
            .load_snapshot_observations_for_edge(snapshot, &row.id)
            .map_err(ScanError::Store)?
            .into_iter()
            .map(|(id, _)| id)
            .collect::<Vec<_>>();
        super::evidence::assess_snapshot_observations(store, &ids, at_ns)
    } else {
        super::evidence::assess_edge(store, row, at_ns)
    }
    .map_err(ScanError::Store)?;
    Ok(CountedEdge {
        supporting_observations: assessment.supporting_ids.len(),
        label: twin_core::score_evidence(assessment.factors)
            .strength
            .label(),
        ebpf_connects: if assessment.factors.source == twin_core::EvidenceSourceKind::Ebpf {
            u64::from(assessment.factors.repeat_count)
        } else {
            0
        },
    })
}

fn emulation_support(layout: &TwinLayout, target: &str) -> usize {
    let Ok(store) = Store::open(&layout.db_file()) else {
        return 0;
    };
    let Ok(mut rows) = store.list_active_edges_from(target) else {
        return 0;
    };
    let Ok(incoming) = store.list_active_edges_to(target) else {
        return 0;
    };
    rows.extend(incoming);
    let mut ids = BTreeSet::new();
    for row in rows {
        let Ok(assessment) =
            super::evidence::assess_edge(&store, &row, TimestampNs::now().as_i64())
        else {
            continue;
        };
        ids.extend(assessment.supporting_ids);
    }
    ids.len()
}

pub fn parse_max_age_seconds(input: &str) -> Result<u64, AppError> {
    super::duration::clock_duration(input)
        .map(|duration| duration.as_secs())
        .map_err(|reason| {
            crate::error::TemporalError::InvalidDuration {
                value: input.into(),
                reason,
            }
            .into()
        })
}
