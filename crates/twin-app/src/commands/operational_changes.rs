use crate::error::TemporalError;
use crate::model::OperationalChange;
use std::collections::BTreeMap;
use twin_core::{
    score_evidence, ChangeCategory, EdgeClass, EvidenceFactors, EvidenceRecency,
    EvidenceSourceKind, NodeId, NodeKind, TimestampNs,
};
use twin_store::{EdgeHistoryRow, NodeHistoryRow, Store};

pub(crate) fn summarize(
    store: &Store,
    nodes: &[NodeHistoryRow],
    edges: &[EdgeHistoryRow],
    now: TimestampNs,
) -> Result<Vec<OperationalChange>, TemporalError> {
    let mut changes = BTreeMap::new();
    for row in nodes {
        let Ok(entity) = row.node_id.parse::<NodeId>() else {
            continue;
        };
        let before = store
            .node_history_before(row)
            .map_err(TemporalError::Store)?;
        let before_meta = before
            .as_ref()
            .and_then(|row| serde_json::from_str::<serde_json::Value>(&row.metadata_json).ok())
            .unwrap_or_default();
        let after_meta: serde_json::Value =
            serde_json::from_str(&row.metadata_json).unwrap_or_default();
        let changed_field = |key: &str| -> Option<String> {
            let old = before_meta.get(key)?.as_str()?;
            let new = after_meta.get(key)?.as_str()?;
            if key == "invocation_id"
                && ![old, new].iter().all(|value| {
                    value.len() == 32
                        && value.bytes().all(|byte| byte.is_ascii_hexdigit())
                        && value.bytes().any(|byte| byte != b'0')
                })
            {
                return None;
            }
            (old != new).then(|| format!("{key} {old} -> {new}"))
        };
        let change = match (entity.kind(), row.change_kind.as_str()) {
            (Some(NodeKind::Process), "new" | "reappeared") => Some((
                ChangeCategory::ProcessStarted,
                "process observed".into(),
                false,
            )),
            (Some(NodeKind::Process), "gone") => Some((
                ChangeCategory::ProcessDisappeared,
                "process absent from a completed scan".into(),
                false,
            )),
            (Some(NodeKind::Service), "changed") => changed_field("invocation_id").map(|detail| {
                (
                    ChangeCategory::ServiceRestarted,
                    format!("likely restarted or reactivated; {detail}; restart cause unknown"),
                    true,
                )
            }),
            (Some(NodeKind::File), "changed") => changed_field("content_hash")
                .map(|detail| (ChangeCategory::ConfigChanged, detail, false)),
            (Some(NodeKind::Package), "changed") => changed_field("version")
                .map(|detail| (ChangeCategory::PackageVersionChanged, detail, false)),
            (Some(NodeKind::Container), "new" | "reappeared") => Some((
                ChangeCategory::ContainerAdded,
                "container observed".into(),
                false,
            )),
            (_, _) if row.node_id.starts_with("k8s:") => Some((
                ChangeCategory::KubernetesObjectChanged,
                before
                    .as_ref()
                    .and_then(|old| super::diff::node_change_summary(old, &row.into()))
                    .unwrap_or_else(|| format!("{} object", row.change_kind)),
                false,
            )),
            _ => None,
        };
        if let Some((category, observation, inferred)) = change {
            let change = change_entry(
                category,
                entity,
                row.recorded_at_ns,
                inferred,
                observation,
                format!(
                    "node_history:{} (collector run {:?})",
                    row.id, row.collector_run_id
                ),
                now,
            );
            changes.insert(
                (category, change.entity.clone(), row.recorded_at_ns, row.id),
                change,
            );
        }
    }
    let mut listener_transitions = std::collections::BTreeSet::new();
    if let Some(first) = edges.first() {
        let baseline = store
            .edges_at(TimestampNs::new(first.recorded_at_ns.saturating_sub(1)))
            .map_err(TemporalError::Store)?;
        let mut listeners: BTreeMap<String, String> = baseline
            .into_iter()
            .filter(|edge| edge.kind == "listens_on")
            .map(|edge| (edge.id, edge.to_node_id))
            .collect();
        let mut by_time = BTreeMap::new();
        for row in edges.iter().filter(|row| row.kind == "listens_on") {
            by_time
                .entry(row.recorded_at_ns)
                .or_insert_with(Vec::new)
                .push(row);
        }
        for (time, rows) in by_time {
            let before: std::collections::BTreeSet<_> = listeners.values().cloned().collect();
            for row in rows {
                if row.state == "gone" {
                    listeners.remove(&row.edge_id);
                } else {
                    listeners.insert(row.edge_id.clone(), row.to_node_id.clone());
                }
            }
            let after: std::collections::BTreeSet<_> = listeners.values().cloned().collect();
            for target in after.difference(&before) {
                listener_transitions.insert((
                    time,
                    target.clone(),
                    ChangeCategory::ListeningPortAdded,
                ));
            }
            for target in before.difference(&after) {
                listener_transitions.insert((
                    time,
                    target.clone(),
                    ChangeCategory::ListeningPortDisappeared,
                ));
            }
        }
    }
    for row in edges {
        let Ok(entity) = row.to_node_id.parse::<NodeId>() else {
            continue;
        };
        let category = match (row.kind.as_str(), row.change_kind.as_str(), entity.kind()) {
            ("listens_on", "new" | "reappeared", Some(NodeKind::Port)) => {
                ChangeCategory::ListeningPortAdded
            }
            ("listens_on", "gone", Some(NodeKind::Port)) => {
                ChangeCategory::ListeningPortDisappeared
            }
            ("connects_to", "new" | "reappeared", Some(NodeKind::Port)) => {
                ChangeCategory::OutboundEndpointAdded
            }
            ("runs_image", "new" | "gone" | "reappeared", Some(NodeKind::Image)) => {
                ChangeCategory::ImageChanged
            }
            _ => continue,
        };
        if matches!(
            category,
            ChangeCategory::ListeningPortAdded | ChangeCategory::ListeningPortDisappeared
        ) && !listener_transitions.contains(&(
            row.recorded_at_ns,
            row.to_node_id.clone(),
            category,
        )) {
            continue;
        }
        let change = change_entry(
            category,
            entity,
            row.recorded_at_ns,
            row.class == "inferred" || category == ChangeCategory::OutboundEndpointAdded,
            format!(
                "{}: {} {} {}",
                row.change_kind, row.from_node_id, row.kind, row.to_node_id
            ),
            format!(
                "edge_history:{} (collector run {:?})",
                row.id, row.collector_run_id
            ),
            now,
        );
        // Process and service projections can describe the same endpoint in one scan.
        changes
            .entry((category, change.entity.clone(), row.recorded_at_ns, 0))
            .or_insert(change);
    }
    Ok(changes.into_values().collect())
}

fn change_entry(
    category: ChangeCategory,
    entity: NodeId,
    recorded_at_ns: i64,
    inferred: bool,
    observation: String,
    source: String,
    now: TimestampNs,
) -> OperationalChange {
    let is_static = matches!(
        category,
        ChangeCategory::ConfigChanged | ChangeCategory::PackageVersionChanged
    );
    let evidence = score_evidence(EvidenceFactors {
        source: if is_static {
            EvidenceSourceKind::Config
        } else {
            EvidenceSourceKind::None
        },
        runtime_confirmed: !is_static,
        static_confirmed: is_static,
        recency: EvidenceRecency::from_timestamps(recorded_at_ns, now.as_i64()),
        repeat_count: 2,
        ..EvidenceFactors::none()
    });
    OperationalChange {
        category,
        entity,
        recorded_at_ns,
        class: if inferred {
            EdgeClass::Inferred
        } else {
            EdgeClass::Observed
        },
        observation,
        source,
        evidence_strength: evidence.strength,
    }
}
