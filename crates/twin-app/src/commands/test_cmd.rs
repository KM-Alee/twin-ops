use std::path::{Path, PathBuf};

use serde::Serialize;
use std::str::FromStr;

use twin_core::{EdgeClass, EvidenceLabel, ObservationId, TimestampNs};
use twin_observation::ObservationKind;
use twin_store::{Store, TestRunRow};
use twin_test::{
    evaluate, parse_file, write_starter, EdgeFact, GraphFacts, NodeFact, TestDocument,
    TestRunReport,
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
    let facts = load_facts(&store)?;
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

fn load_facts(store: &Store) -> Result<GraphFacts, AppError> {
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
    for kind in ["depends_on", "connects_to"] {
        for row in store.list_edges_by_kind(kind).map_err(ScanError::Store)? {
            if row.state == "gone" {
                continue;
            }
            edges.push(EdgeFact {
                from: row.from_node_id.clone(),
                to: row.to_node_id.clone(),
                kind: row.kind.clone(),
                evidence: evidence_label(store, &row, dropped)?,
                stale: row.state == "stale",
            });
        }
    }
    Ok(GraphFacts { nodes, edges })
}

fn evidence_label(
    store: &Store,
    row: &twin_store::EdgeRow,
    dropped: bool,
) -> Result<EvidenceLabel, AppError> {
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
    Ok(connect_evidence_strength(ebpf_connects, socket_table, config_only, dropped).label())
}
