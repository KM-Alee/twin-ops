use std::collections::HashMap;
use std::convert::TryFrom;
use std::path::Path;

use twin_collectors::{
    ProcessBatch, ProcessCollector, ProcessWarning, ProcessWarningKind, COLLECTOR_NAME,
};
use twin_core::{EdgeId, EdgeKind, GraphEdge, GraphNode, NodeId, ObservationId, TimestampNs};
use twin_observation::{Observation, ObservationKind, Pipeline};
use twin_store::{CollectorRunRow, Store};

use crate::error::{AppError, ScanError};
use crate::model::{ScanResult, ScanWarning, ScanWarningDetail};
use crate::paths::{resolve_command_paths, TwinLayout};
use crate::ScanRequest;

pub fn run_home(request: ScanRequest, proc_root: &Path) -> Result<ScanResult, AppError> {
    let paths = resolve_command_paths(request.config_override.as_deref())?;
    scan_at(&paths.layout, &request, proc_root, &paths.db_path)
}

pub fn run(
    layout: &TwinLayout,
    request: &ScanRequest,
    proc_root: &Path,
) -> Result<ScanResult, AppError> {
    scan_at(layout, request, proc_root, &layout.db_file())
}

fn scan_at(
    layout: &TwinLayout,
    request: &ScanRequest,
    proc_root: &Path,
    db_path: &Path,
) -> Result<ScanResult, AppError> {
    let _ = layout.config_file(request.config_override.as_deref());
    if !db_path.exists() {
        return Err(ScanError::DatabaseNotInitialized.into());
    }
    let started_at = TimestampNs::now();
    let batch = ProcessCollector::new(proc_root).collect(started_at)?;
    persist_scan(db_path, batch)
}

fn persist_scan(db_path: &Path, mut batch: ProcessBatch) -> Result<ScanResult, AppError> {
    let mut store = Store::open(db_path).map_err(ScanError::StoreOpen)?;
    if !store.is_initialized().map_err(ScanError::Store)? {
        return Err(ScanError::DatabaseNotInitialized.into());
    }

    let pipeline = Pipeline::default();
    let mut observations: Vec<Observation> = Vec::new();
    for raw in batch.drain_observations() {
        observations.push(pipeline.process(raw).map_err(ScanError::Observation)?);
    }

    let parent_obs_by_child = parent_observation_ids(&observations);
    let scan_time = batch.ended_at();
    let mut process_count = 0usize;
    let mut parent_edge_count = 0usize;

    store
        .with_transaction(|store| {
            let run_id = store.insert_collector_run(&CollectorRunRow {
                id: None,
                collector: COLLECTOR_NAME.to_string(),
                started_at_ns: batch.started_at().as_i64(),
                ended_at_ns: batch.ended_at().as_i64(),
                status: "success".to_string(),
                observation_count: observations.len() as i64,
                warning_count: batch.warnings().len() as i64,
                error_message: None,
            })?;

            for obs in &observations {
                store.insert_observation_typed_for_run(obs, run_id)?;
            }

            for record in batch.records() {
                let node_id = NodeId::process(record.pid());
                let existing = store.get_node_typed(&node_id)?;
                let node =
                    GraphNode::process(record.pid(), record.label(), scan_time, existing.as_ref());
                store.upsert_node_typed(&node)?;
                process_count += 1;

                if let Some(exe) = record.exe() {
                    let exe_str = exe.to_string_lossy();
                    let file_id = NodeId::file(&exe_str);
                    let file_existing = store.get_node_typed(&file_id)?;
                    let file_node = GraphNode::file(&exe_str, scan_time, file_existing.as_ref());
                    store.upsert_node_typed(&file_node)?;
                }

                let Some(ppid) = record.ppid() else {
                    continue;
                };
                if ppid == 0 {
                    continue;
                }
                let parent_id = NodeId::process(ppid);
                let child_id = NodeId::process(record.pid());
                let parent_existing = store.get_node_typed(&parent_id)?;
                if parent_existing.is_none() {
                    let parent_node =
                        GraphNode::process(ppid, format!("pid:{ppid}"), scan_time, None);
                    store.upsert_node_typed(&parent_node)?;
                }
                let edge_id = EdgeId::new(&parent_id, EdgeKind::ParentOf, &child_id);
                let edge_existing = store.get_edge(edge_id.as_str())?;
                let existing_edge = edge_existing
                    .as_ref()
                    .and_then(|row| GraphEdge::try_from(row).ok());
                let edge = GraphEdge::observed_parent(
                    &parent_id,
                    &child_id,
                    scan_time,
                    existing_edge.as_ref(),
                );
                store.upsert_edge_typed(&edge)?;
                parent_edge_count += 1;

                if let Some(obs_id) = parent_obs_by_child.get(&record.pid()) {
                    store.link_edge_observation(
                        edge.id().as_str(),
                        &obs_id.to_string(),
                        "support",
                    )?;
                }
            }
            Ok(())
        })
        .map_err(ScanError::Store)?;

    let collector_warnings = batch.warnings();
    Ok(ScanResult {
        started_at_ns: batch.started_at().as_i64(),
        ended_at_ns: batch.ended_at().as_i64(),
        process_count,
        parent_edge_count,
        observation_count: observations.len(),
        warning_count: collector_warnings.len(),
        warnings: aggregate_warnings(collector_warnings),
        warning_details: detailed_warnings(collector_warnings),
    })
}

fn parent_observation_ids(observations: &[Observation]) -> HashMap<u32, ObservationId> {
    let mut map = HashMap::new();
    for obs in observations {
        if obs.kind() != ObservationKind::ProcessParentSeen {
            continue;
        }
        let Some(child) = obs.object() else {
            continue;
        };
        let Some(pid) = child.process_pid() else {
            continue;
        };
        map.insert(pid, obs.id());
    }
    map
}

fn aggregate_warnings(warnings: &[ProcessWarning]) -> Vec<ScanWarning> {
    let mut vanished = 0usize;
    let mut permission = 0usize;
    let mut malformed = 0usize;
    let mut exe = 0usize;
    for w in warnings {
        match w.kind() {
            ProcessWarningKind::Vanished => vanished += 1,
            ProcessWarningKind::PermissionDenied => permission += 1,
            ProcessWarningKind::Malformed => malformed += 1,
            ProcessWarningKind::ExeUnreadable => exe += 1,
        }
    }
    let mut out = Vec::new();
    push_aggregate(&mut out, ProcessWarningKind::Vanished, vanished);
    push_aggregate(&mut out, ProcessWarningKind::PermissionDenied, permission);
    push_aggregate(&mut out, ProcessWarningKind::Malformed, malformed);
    push_aggregate(&mut out, ProcessWarningKind::ExeUnreadable, exe);
    out
}

fn push_aggregate(out: &mut Vec<ScanWarning>, kind: ProcessWarningKind, count: usize) {
    if count > 0 {
        out.push(ScanWarning {
            kind: kind.aggregate_key().to_string(),
            count,
        });
    }
}

fn detailed_warnings(warnings: &[ProcessWarning]) -> Vec<ScanWarningDetail> {
    warnings
        .iter()
        .filter(|w| w.kind().includes_json_detail())
        .map(|w| ScanWarningDetail {
            kind: w.kind().detail_key().to_string(),
            path: w.path().display().to_string(),
            detail: w.detail().to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use twin_collectors::ProcessWarningKind;

    #[test]
    fn bulk_coverage_gaps_omit_per_pid_json_details() {
        assert!(!ProcessWarningKind::ExeUnreadable.includes_json_detail());
        assert!(!ProcessWarningKind::PermissionDenied.includes_json_detail());
        assert!(ProcessWarningKind::Vanished.includes_json_detail());
        assert!(ProcessWarningKind::Malformed.includes_json_detail());
    }
}
