use std::path::Path;

use twin_collectors::{collect_cron, CronEntry};
use twin_core::{
    GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState, ObservationId,
    TimestampNs,
};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RedactionState,
};
use twin_store::{Store, StoreError};

use crate::model::{ScanWarning, ScanWarningDetail};

const SOURCE: &str = "cron_file";
/// Services that run crontab jobs, in the order distributions name them.
const CRON_DAEMONS: [&str; 3] = ["cron.service", "cronie.service", "crond.service"];

#[derive(Default)]
pub(crate) struct CronScanNotes {
    pub jobs: usize,
    pub warnings: Vec<ScanWarning>,
    pub details: Vec<ScanWarningDetail>,
}

/// Records every crontab entry as a job node owned by the cron daemon's service. Only the
/// schedule, user, program names and typed operations are kept, never the command text.
pub(crate) fn persist_cron_in_scan(
    store: &mut Store,
    history: &mut super::scan_history::ScanHistorySession,
    edge_cache: &mut super::scan::ScanEdgeCache,
    root: &Path,
    scan_time: TimestampNs,
) -> Result<CronScanNotes, StoreError> {
    let batch = collect_cron(root);
    if !batch.is_complete() {
        history.uncertain_source(SOURCE);
    }
    let runner = cron_daemon(store)?;
    let mut notes = CronScanNotes::default();
    for entry in batch.entries() {
        let host_path = host_path(root, entry.path());
        let id = NodeId::cron_job(&host_path, entry.line());
        let facts = super::preflight::scheduled_facts(entry.command());
        let metadata = serde_json::json!({
            "source": SOURCE,
            "schedule": entry.schedule(),
            "user": entry.user(),
            "programs": facts.programs,
            "operations": facts.operations,
        });
        let existing = store.get_node_typed(&id)?;
        let node = GraphNode::from_parts(GraphNodeParts {
            id: id.clone(),
            kind: NodeKind::CronJob,
            label: job_label(&host_path, entry),
            state: NodeState::Active,
            first_seen: existing
                .as_ref()
                .map_or(scan_time, |node| node.first_seen()),
            last_seen: scan_time,
            valid_from: existing
                .as_ref()
                .map_or(scan_time, |node| node.valid_from()),
            valid_to: None,
            metadata: GraphMetadata::from_json(metadata.to_string()),
        });
        super::scan_history::upsert_node(store, history, &node)?;
        let observation = job_observation(&id, &host_path, entry, &metadata, scan_time)?;
        match history.collector_run_id() {
            Some(run) => store.insert_observation_typed_for_run(&observation, run)?,
            None => store.insert_observation_typed(&observation)?,
        }
        if let Some(runner) = &runner {
            let existing = super::scan::load_existing_edge(
                store,
                edge_cache,
                runner,
                twin_core::EdgeKind::Owns,
                &id,
            )?;
            let edge = twin_core::GraphEdge::inferred_service_owns(
                runner,
                &id,
                scan_time,
                existing.as_ref(),
            )
            .with_metadata(GraphMetadata::from_json(
                serde_json::json!({"inference": "cron_runner", "source": SOURCE}).to_string(),
            ));
            let observation_id: ObservationId = observation.id();
            super::scan::upsert_edge_with_link(
                store,
                history,
                &edge,
                Some((&observation_id, "support")),
            )?;
        }
        notes.jobs += 1;
    }
    for warning in batch.warnings() {
        let kind = format!("cron_{:?}", warning.kind()).to_ascii_lowercase();
        match notes.warnings.iter_mut().find(|known| known.kind == kind) {
            Some(known) => known.count += 1,
            None => notes.warnings.push(ScanWarning {
                kind: kind.clone(),
                count: 1,
            }),
        }
        notes.details.push(ScanWarningDetail {
            kind,
            path: host_path(root, warning.path()),
            detail: warning.detail().to_string(),
        });
    }
    Ok(notes)
}

/// The first known cron daemon service, preferring one that is running.
fn cron_daemon(store: &Store) -> Result<Option<NodeId>, StoreError> {
    let mut known = None;
    for unit in CRON_DAEMONS {
        let id = NodeId::service(unit);
        let Some(node) = store.get_node_typed(&id)? else {
            continue;
        };
        if node.state() == NodeState::Active && super::evidence::is_service_running(store, &node)? {
            return Ok(Some(id));
        }
        known.get_or_insert(id);
    }
    Ok(known)
}

fn job_observation(
    id: &NodeId,
    host_path: &str,
    entry: &CronEntry,
    facts: &serde_json::Value,
    scan_time: TimestampNs,
) -> Result<Observation, StoreError> {
    let metadata =
        ObservationMetadata::from_json(&facts.to_string()).map_err(|error| StoreError::Decode {
            detail: error.to_string(),
        })?;
    Ok(Observation::from_parts(
        ObservationId::new(),
        ObservationSource::CronFile,
        ObservationKind::CronJobSeen,
        Some(id.clone()),
        None,
        scan_time,
        Some(RawEvidenceRef::new(format!("{host_path}:{}", entry.line()))),
        ConfidenceHint::High,
        RedactionState::None,
        metadata,
    ))
}

fn job_label(host_path: &str, entry: &CronEntry) -> String {
    let name = Path::new(host_path)
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| host_path.to_string());
    match entry.line() {
        0 => format!("{} ({})", name, entry.schedule()),
        line => format!("{name}:{line} ({})", entry.schedule()),
    }
}

fn host_path(root: &Path, path: &Path) -> String {
    Path::new("/")
        .join(path.strip_prefix(root).unwrap_or(path))
        .display()
        .to_string()
}
