use std::ffi::CString;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::Serialize;
use twin_core::{EdgeClass, EvidenceLabel, NodeId, ObservationId, RiskLevel, TimestampNs};
use twin_observation::ObservationKind;
use twin_store::{EdgeRow, Store, TestRunRow};
use twin_test::{
    evaluate, parse_file, write_starter, CheckKind, DiskFact, EdgeFact, EmulateAction,
    EmulationFact, EndpointFact, GraphFacts, NodeFact, TestDocument, TestRunReport, UnknownFact,
};
use uuid::Uuid;

use crate::commands::ebpf_tcp::{connect_evidence_strength, tcp_drop_count};
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

pub fn init_file(request: &TestInitRequest) -> Result<TestInitOutcome, AppError> {
    write_starter(&request.path, request.force).map_err(TestCmdError::Test)?;
    Ok(TestInitOutcome {
        path: request.path.clone(),
    })
}

pub fn lint_file(path: &Path) -> Result<TestDocument, AppError> {
    Ok(parse_file(path).map_err(TestCmdError::Test)?)
}

pub fn run_file(layout: &TwinLayout, path: &Path) -> Result<TestRunReport, AppError> {
    let document = lint_file(path)?;
    let db = layout.db_file();
    if !db.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let mut store = Store::open(&db).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let facts = load_facts(layout, &store, &document)?;
    let report = evaluate(&document, &facts);
    let report_json = serde_json::to_string(&report).map_err(|err| TestCmdError::Encode {
        reason: err.to_string(),
    })?;
    store
        .insert_test_run(&TestRunRow {
            id: Uuid::new_v4().to_string(),
            name: report.name.clone(),
            file_path: path.display().to_string(),
            started_at_ns: TimestampNs::now().as_i64(),
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
) -> Result<GraphFacts, AppError> {
    let dropped = tcp_drop_count(store).map_err(ScanError::Store)? > 0;
    let mut nodes = Vec::new();
    for row in store.list_nodes().map_err(ScanError::Store)? {
        if row.state == "gone" {
            continue;
        }
        nodes.push(NodeFact {
            id: row.id,
            stale: row.state == "stale",
        });
    }
    let mut edges = Vec::new();
    let mut endpoints = Vec::new();
    for kind in ["depends_on", "connects_to"] {
        for row in store.list_edges_by_kind(kind).map_err(ScanError::Store)? {
            if row.state == "gone" {
                continue;
            }
            let counted = count_edge(store, &row, dropped)?;
            if row.kind == "connects_to" {
                if let Some(endpoint) = endpoint_label(&row.to_node_id) {
                    endpoints.push(EndpointFact {
                        endpoint,
                        evidence: endpoint_evidence(&row.from_node_id, counted.ebpf_connects),
                    });
                }
            }
            edges.push(EdgeFact {
                from: row.from_node_id.clone(),
                to: row.to_node_id.clone(),
                kind: row.kind.clone(),
                evidence: counted.label,
                stale: row.state == "stale",
            });
        }
    }
    Ok(GraphFacts {
        nodes,
        edges,
        endpoints,
        disks: disk_facts(document),
        unknowns: unknown_facts(store)?,
        emulations: emulation_facts(layout, document),
    })
}

struct CountedEdge {
    label: EvidenceLabel,
    ebpf_connects: u64,
}

fn endpoint_label(port_id: &str) -> Option<String> {
    port_id.strip_prefix("port:tcp:").map(str::to_string)
}

fn endpoint_evidence(from: &str, ebpf_connects: u64) -> String {
    if ebpf_connects > 0 {
        format!("eBPF observed {from} connecting {ebpf_connects} times")
    } else {
        format!("socket table observed {from} connecting")
    }
}

fn disk_facts(document: &TestDocument) -> Vec<DiskFact> {
    let mut disks = Vec::new();
    for check in &document.checks {
        if let CheckKind::Disk { mount, .. } = &check.kind {
            if let Some(used_percent) = mount_used_percent(mount) {
                disks.push(DiskFact {
                    mount: mount.clone(),
                    used_percent,
                });
            }
        }
    }
    disks
}

fn mount_used_percent(mount: &str) -> Option<u8> {
    let path = CString::new(mount).ok()?;
    let mut stat = unsafe { std::mem::zeroed::<libc::statvfs>() };
    // SAFETY: `path` is NUL-terminated. `stat` is a writable statvfs buffer.
    // statvfs only reads filesystem metadata for the mount.
    let rc = unsafe { libc::statvfs(path.as_ptr(), &mut stat) };
    if rc != 0 {
        return None;
    }
    let blocks = stat.f_blocks as u128;
    if blocks == 0 {
        return None;
    }
    let available = stat.f_bavail as u128;
    let used = blocks.saturating_sub(available);
    let percent = used.saturating_mul(100) / blocks;
    u8::try_from(percent.min(100)).ok()
}

fn unknown_facts(store: &Store) -> Result<Vec<UnknownFact>, AppError> {
    let report = crate::commands::coverage::load(store)?;
    Ok(report
        .unknown_lines()
        .into_iter()
        .map(|line| UnknownFact {
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
            EmulationFact {
                action: action.as_str().to_string(),
                target: target.to_string(),
                risk: result.risk.level,
                evidence_label,
                evidence,
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
    }
}

fn count_edge(store: &Store, row: &EdgeRow, dropped: bool) -> Result<CountedEdge, AppError> {
    let links = store
        .list_observations_for_edge(&row.id)
        .map_err(ScanError::Store)?;
    let mut ebpf_connects = 0u64;
    let mut socket_table =
        row.kind == "connects_to" && row.class == EdgeClass::Observed.to_string();
    let mut config_only = row.kind == "depends_on" && row.class == EdgeClass::Observed.to_string();
    for (obs_id, _) in links {
        let Ok(id) = ObservationId::from_str(&obs_id) else {
            continue;
        };
        let Ok(Some(obs)) = store.get_observation_typed(id) else {
            continue;
        };
        match obs.kind() {
            ObservationKind::EbpfConnect => ebpf_connects += 1,
            ObservationKind::TcpConnectionSeen
            | ObservationKind::TcpSocketSeen
            | ObservationKind::UnixConnectionSeen
            | ObservationKind::UnixSocketSeen => socket_table = true,
            ObservationKind::SystemdUnitRequires
            | ObservationKind::SystemdUnitWants
            | ObservationKind::ServiceConfiguredByFile => config_only = true,
            _ => {}
        }
    }
    Ok(CountedEdge {
        label: connect_evidence_strength(ebpf_connects, socket_table, config_only, dropped).label(),
        ebpf_connects,
    })
}
