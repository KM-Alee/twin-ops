use std::collections::BTreeSet;
use std::path::Path;

use twin_container::containerd::{read_containerd, NamespaceSnapshot, CONTAINERD_SOCKET};
use twin_core::{
    EdgeKind, GraphEdge, GraphMetadata, GraphNode, GraphNodeParts, NodeId, NodeKind, NodeState,
    ObservationId, TimestampNs,
};
use twin_observation::{
    ConfidenceHint, Observation, ObservationKind, ObservationMetadata, ObservationSource,
    RawEvidenceRef, RedactionState,
};
use twin_store::{CollectorRunRow, Store, StoreError};

use super::scan::ScanEdgeCache;
use super::scan_history::ScanHistorySession;
use crate::model::{ScanWarning, ScanWarningDetail};

#[derive(Default)]
pub(crate) struct ContainerdScanNotes {
    pub unavailable: bool,
    pub incomplete: bool,
    pub observation_count: usize,
    pub warnings: Vec<ScanWarning>,
    pub details: Vec<ScanWarningDetail>,
}

pub(crate) fn persist(
    store: &mut Store,
    history: &mut ScanHistorySession,
    cache: &mut ScanEdgeCache,
    now: TimestampNs,
    fixture: Option<&Path>,
    proc_root: &Path,
) -> Result<ContainerdScanNotes, StoreError> {
    let mut notes = ContainerdScanNotes::default();
    let snapshot = read_containerd(&crate::paths::host_path(CONTAINERD_SOCKET), fixture);
    let existing_sources: BTreeSet<String> = store
        .list_unfinalized_nodes()?
        .iter()
        .filter_map(|row| {
            let metadata: serde_json::Value = serde_json::from_str(&row.metadata_json).ok()?;
            let source = metadata.get("source")?.as_str()?;
            source
                .starts_with("containerd:")
                .then(|| source.to_string())
        })
        .collect();
    match snapshot {
        Err(error) => {
            notes.unavailable = true;
            for source in existing_sources {
                history.skip_source(&source);
            }
            notes.details.push(detail(error.to_string()));
        }
        Ok(snapshot) => {
            notes.incomplete = !snapshot.is_complete;
            let present: BTreeSet<String> = snapshot
                .namespace_names
                .iter()
                .map(|namespace| format!("containerd:{namespace}"))
                .collect();
            for source in existing_sources.difference(&present) {
                history.complete_source(source);
            }
            for namespace in snapshot.namespaces {
                let source = format!("containerd:{}", namespace.namespace);
                if !namespace.warnings.is_empty() {
                    history.uncertain_source(&source);
                } else {
                    history.complete_source(&source);
                }
                for warning in &namespace.warnings {
                    notes.details.push(detail(warning.clone()));
                }
                persist_namespace(
                    store, history, cache, now, &namespace, proc_root, &mut notes,
                )?;
            }
        }
    }
    if !notes.details.is_empty() {
        notes.warnings.push(ScanWarning {
            kind: if notes.unavailable {
                "containerd_unavailable"
            } else {
                "containerd_incomplete"
            }
            .into(),
            count: notes.details.len(),
        });
    }
    store.insert_collector_run(&CollectorRunRow {
        id: None,
        collector: "containerd".into(),
        started_at_ns: now.as_i64(),
        ended_at_ns: TimestampNs::now().as_i64(),
        status: if notes.unavailable {
            "unavailable"
        } else if notes.incomplete {
            "degraded"
        } else {
            "success"
        }
        .into(),
        observation_count: notes.observation_count as i64,
        warning_count: notes.details.len() as i64,
        error_message: notes.details.first().map(|row| row.detail.clone()),
        metadata_json: "{}".into(),
    })?;
    Ok(notes)
}

fn detail(message: String) -> ScanWarningDetail {
    ScanWarningDetail {
        kind: "containerd".into(),
        path: CONTAINERD_SOCKET.into(),
        detail: message,
    }
}

fn persist_namespace(
    store: &mut Store,
    history: &mut ScanHistorySession,
    cache: &mut ScanEdgeCache,
    now: TimestampNs,
    namespace: &NamespaceSnapshot,
    proc_root: &Path,
    notes: &mut ContainerdScanNotes,
) -> Result<(), StoreError> {
    let source = format!("containerd:{}", namespace.namespace);
    if let Some(images) = &namespace.images {
        for image in images {
            let id = NodeId::containerd_image(&namespace.namespace, &image.name).map_err(decode)?;
            let metadata = serde_json::json!({"source":source,"runtime":"containerd","namespace":namespace.namespace,"digest":image.digest});
            persist_node(
                store,
                history,
                id.clone(),
                NodeKind::Image,
                &image.name,
                now,
                metadata.clone(),
            )?;
            observe(
                store,
                history,
                (id, None),
                ObservationKind::ContainerImageSeen,
                now,
                (&namespace.namespace, "images.list"),
                metadata,
            )?;
            notes.observation_count += 1;
        }
    }
    let Some(containers) = &namespace.containers else {
        return Ok(());
    };
    for container in containers {
        let id = NodeId::containerd(&namespace.namespace, &container.id).map_err(decode)?;
        let previous = store.get_node_typed(&id)?;
        let old_created = previous
            .as_ref()
            .and_then(|row| serde_json::from_str::<serde_json::Value>(row.metadata().as_str()).ok())
            .and_then(|meta| meta.get("created_at_ns").and_then(|v| v.as_i64()));
        let recreated = old_created
            .zip(container.created_at_ns)
            .is_some_and(|(old, new)| old != new)
            || previous
                .as_ref()
                .is_some_and(|row| row.state() == NodeState::Gone);
        if recreated {
            let mut edges = store.list_edges_from(id.as_str())?;
            edges.extend(store.list_edges_to(id.as_str())?);
            for row in edges {
                let edge = GraphEdge::try_from(&row)?;
                store.mark_edges_missing(
                    &[row.id],
                    now.as_i64(),
                    false,
                    history.collector_run_id(),
                )?;
                store.reset_edge_incarnation(edge.id(), now)?;
            }
            store.reset_node_incarnation(&id, now)?;
        }
        let task = namespace
            .tasks
            .as_ref()
            .and_then(|tasks| tasks.iter().find(|task| task.container_id == container.id));
        let is_running = task
            .and_then(|task| task.state.is_running())
            .or_else(|| namespace.tasks.as_ref().map(|_| false));
        let metadata = serde_json::json!({"source":source,"runtime":"containerd","namespace":namespace.namespace,"containerd_id":container.id,"created_at_ns":container.created_at_ns.or(old_created),"created_identity_known":container.created_at_ns.is_some(),"running_known":is_running.is_some(),"is_running":is_running,"health_status":"unknown"});
        persist_node(
            store,
            history,
            id.clone(),
            NodeKind::Container,
            &format!("{}/{}", namespace.namespace, container.id),
            now,
            metadata.clone(),
        )?;
        let observation = observe(
            store,
            history,
            (id.clone(), None),
            ObservationKind::ContainerSeen,
            now,
            (&namespace.namespace, "containers.list"),
            metadata,
        )?;
        notes.observation_count += 1;
        if let Some(image) = &container.image {
            let image_id = NodeId::containerd_image(&namespace.namespace, image).map_err(decode)?;
            if store.get_node_typed(&image_id)?.is_none() {
                persist_node(
                    store,
                    history,
                    image_id.clone(),
                    NodeKind::Image,
                    image,
                    now,
                    serde_json::json!({"source":source,"runtime":"containerd","namespace":namespace.namespace}),
                )?;
            } else if let Some(node) = store.get_node_typed(&image_id)? {
                super::scan_history::upsert_node(store, history, &node)?;
            }
            let edge = GraphEdge::observed_runs_image(
                &id,
                &image_id,
                now,
                super::scan::load_existing_edge(store, cache, &id, EdgeKind::RunsImage, &image_id)?
                    .as_ref(),
            )
            .with_metadata(GraphMetadata::from_json(
                serde_json::json!({"source":source}).to_string(),
            ));
            super::scan::upsert_edge_with_link(
                store,
                history,
                &edge,
                Some((&observation, "runtime")),
            )?;
        }
        if let Some(task) = task {
            let metadata = serde_json::json!({"source":source,"namespace":namespace.namespace,"pid":task.pid,"state":task.state.as_str()});
            let task_observation = observe(
                store,
                history,
                (id.clone(), task.pid.map(NodeId::process)),
                ObservationKind::ContainerTaskSeen,
                now,
                (&namespace.namespace, "tasks.list"),
                metadata,
            )?;
            notes.observation_count += 1;
            if let Some(pid) = task.pid {
                let process = NodeId::process(pid);
                if process_matches(store, &process, proc_root, pid)? {
                    let edge = GraphEdge::observed_container_owns_process(
                        &id,
                        &process,
                        now,
                        super::scan::load_existing_edge(
                            store,
                            cache,
                            &id,
                            EdgeKind::Owns,
                            &process,
                        )?
                        .as_ref(),
                    )
                    .with_metadata(GraphMetadata::from_json(
                        serde_json::json!({"source":source}).to_string(),
                    ));
                    super::scan::upsert_edge_with_link(
                        store,
                        history,
                        &edge,
                        Some((&task_observation, "runtime")),
                    )?;
                } else {
                    history.uncertain_source(&source);
                    notes.incomplete = true;
                    notes.details.push(detail(
                        "task ownership unavailable: process incarnation could not be verified"
                            .into(),
                    ));
                }
            }
        }
    }
    Ok(())
}

fn process_matches(
    store: &Store,
    process: &NodeId,
    root: &Path,
    pid: u32,
) -> Result<bool, StoreError> {
    let Some(node) = store.get_node_typed(process)? else {
        return Ok(false);
    };
    let metadata: serde_json::Value =
        serde_json::from_str(node.metadata().as_str()).unwrap_or_default();
    let expected = metadata.get("start_ticks").and_then(|v| v.as_u64());
    let actual = std::fs::read_to_string(root.join(pid.to_string()).join("stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(") ")
                .and_then(|(_, tail)| tail.split_whitespace().nth(19))
                .and_then(|ticks| ticks.parse::<u64>().ok())
        });
    Ok(node.state() == NodeState::Active
        && expected.is_some()
        && expected == actual
        && metadata
            .get("start_identity_known")
            .and_then(|v| v.as_bool())
            == Some(true))
}

fn persist_node(
    store: &mut Store,
    history: &mut ScanHistorySession,
    id: NodeId,
    kind: NodeKind,
    label: &str,
    now: TimestampNs,
    metadata: serde_json::Value,
) -> Result<(), StoreError> {
    let existing = store.get_node_typed(&id)?;
    let node = GraphNode::from_parts(GraphNodeParts {
        id,
        kind,
        label: label.into(),
        state: NodeState::Active,
        first_seen: existing.as_ref().map_or(now, GraphNode::first_seen),
        last_seen: now,
        valid_from: existing.as_ref().map_or(now, GraphNode::valid_from),
        valid_to: None,
        metadata: GraphMetadata::from_json(metadata.to_string()),
    });
    super::scan_history::upsert_node(store, history, &node)
}

fn observe(
    store: &mut Store,
    history: &ScanHistorySession,
    identities: (NodeId, Option<NodeId>),
    kind: ObservationKind,
    now: TimestampNs,
    provenance: (&str, &str),
    metadata: serde_json::Value,
) -> Result<ObservationId, StoreError> {
    let (subject, object) = identities;
    let (namespace, operation) = provenance;
    let id = ObservationId::new();
    let metadata = ObservationMetadata::from_json(&metadata.to_string()).map_err(decode)?;
    let observation = Observation::from_parts(
        id,
        ObservationSource::Containerd,
        kind,
        Some(subject),
        object,
        now,
        Some(RawEvidenceRef::new(format!(
            "containerd:{namespace}/{operation}"
        ))),
        ConfidenceHint::High,
        RedactionState::None,
        metadata,
    );
    if let Some(run) = history.collector_run_id() {
        store.insert_observation_typed_for_run(&observation, run)?;
    } else {
        store.insert_observation_typed(&observation)?;
    }
    Ok(id)
}

fn decode(error: impl std::fmt::Display) -> StoreError {
    StoreError::Decode {
        detail: error.to_string(),
    }
}
