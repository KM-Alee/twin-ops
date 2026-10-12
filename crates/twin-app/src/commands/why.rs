use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;

use twin_core::{EdgeKind, EvidenceStrength, GraphEdge, NodeId};
use twin_store::Store;

use crate::commands::resolve_service::{resolve_service_target, ServiceNotFoundContext};
use crate::model::{WhyObservation, WhyProofChain, WhyRelationship, WhyResult};
use crate::{AppError, GraphError, TwinLayout};

#[derive(Debug, Clone)]
pub struct WhyRequest {
    pub config_override: Option<std::path::PathBuf>,
    pub target: Option<NodeId>,
    pub target_query: Option<String>,
}

pub fn run_home(request: WhyRequest) -> Result<WhyResult, AppError> {
    let paths = crate::paths::resolve_command_paths(request.config_override.as_deref())?;
    run(&paths.layout, &request)
}

pub fn run(layout: &TwinLayout, request: &WhyRequest) -> Result<WhyResult, AppError> {
    crate::config_io::load(layout, request.config_override.as_deref())?;
    if !layout.db_file().exists() {
        return Err(GraphError::DatabaseNotInitialized.into());
    }
    let store = Store::open(&layout.db_file()).map_err(GraphError::StoreOpen)?;
    if !store.is_initialized().map_err(GraphError::Store)? {
        return Err(GraphError::DatabaseNotInitialized.into());
    }
    let target = match &request.target {
        Some(id) => id.clone(),
        None => resolve_service_target(
            &store,
            request
                .target_query
                .as_deref()
                .ok_or(GraphError::MissingQuery)?,
            ServiceNotFoundContext::Graph {
                detail: String::new(),
            },
        )?,
    };
    let node = store
        .get_node_typed(&target)
        .map_err(GraphError::Store)?
        .ok_or_else(|| GraphError::NodeNotFound { id: target.clone() })?;
    let at_ns = super::evidence::now_ns();
    let coverage = super::coverage::load(&store)?;
    let mut relationships = Vec::new();
    let (rows, traversal_capped) = proof_rows(&store, &target)?;
    for row in rows {
        let edge = GraphEdge::try_from(&row).map_err(GraphError::Store)?;
        let mut observations = Vec::new();
        let mut unknowns = Vec::new();
        for (id, _) in store
            .list_observations_for_edge(&row.id)
            .map_err(GraphError::Store)?
        {
            match store.get_observation(&id).map_err(GraphError::Store)? {
                Some(observation) => {
                    let raw_ref =
                        serde_json::from_str::<serde_json::Value>(&observation.metadata_json)
                            .ok()
                            .and_then(|metadata| {
                                metadata
                                    .get("__raw_ref")
                                    .and_then(|value| value.as_str())
                                    .map(str::to_string)
                            });
                    let facts = safe_facts(&observation.metadata_json);
                    observations.push(WhyObservation {
                        id,
                        source: observation.source,
                        kind: observation.kind,
                        timestamp_ns: observation.timestamp_ns,
                        raw_ref,
                        facts,
                        repeat_count: 1,
                        first_timestamp_ns: observation.timestamp_ns,
                    });
                }
                None => unknowns.push(format!("observation {id} is no longer retained")),
            }
        }
        observations.sort_by(|a, b| {
            a.timestamp_ns
                .cmp(&b.timestamp_ns)
                .then_with(|| a.id.cmp(&b.id))
        });
        let mut observations = collapse_repeated(observations);
        let omitted = observations.len().saturating_sub(4);
        if omitted > 0 {
            observations.drain(..omitted);
            unknowns.push(format!("{omitted} earlier observations omitted from this explanation; score uses all retained evidence"));
        }
        if observations.is_empty() {
            unknowns.push(
                "no supporting observations are retained; this relationship is unverified".into(),
            );
        }
        let ids = observations
            .iter()
            .map(|observation| observation.id.clone())
            .collect::<Vec<_>>();
        let conflicting = super::evidence::observations_conflict(&store, &ids)
            .map_err(GraphError::Store)?
            || super::evidence::ownership_conflict(&store, edge.from())
                .map_err(GraphError::Store)?
            || super::evidence::ownership_conflict(&store, edge.to()).map_err(GraphError::Store)?;
        if conflicting {
            unknowns
                .push("competing active service ownership claims lower evidence confidence".into());
        }
        let mut assessment =
            super::evidence::assess_edge(&store, &row, at_ns).map_err(GraphError::Store)?;
        assessment.factors.conflicting |= conflicting;
        if assessment.missing > 0 {
            unknowns.push(format!(
                "{} supporting observations are unavailable",
                assessment.missing
            ));
        }
        if matches!(
            assessment.factors.recency,
            twin_core::EvidenceRecency::Stale | twin_core::EvidenceRecency::Unknown
        ) {
            unknowns.push("supporting evidence is old or its observation time is unknown".into());
        }
        let strength = if assessment.supporting_ids.is_empty() {
            EvidenceStrength::new(0)
        } else {
            twin_core::score_evidence(assessment.factors).strength
        };
        relationships.push(WhyRelationship {
            id: row.id,
            from: edge.from().to_string(),
            to: edge.to().to_string(),
            kind: edge.kind().to_string(),
            class: edge.class().to_string(),
            evidence_score: strength.score(),
            evidence_strength: strength.label().to_string(),
            observations,
            unknowns,
        });
    }
    relationships.sort_by(|a, b| a.id.cmp(&b.id));
    let mut unknowns = if relationships.is_empty() {
        vec!["no stored relationships explain this node".into()]
    } else {
        Vec::new()
    };
    // Host-wide coverage gaps apply to every relationship; list them once.
    unknowns.extend(coverage.unknown_lines().into_iter().map(|line| line.detail));
    if traversal_capped {
        unknowns.push(
            "proof neighborhood capped at 64 relationships and 32 nodes; explanation is partial"
                .into(),
        );
    }
    let mut proof_chains = Vec::new();
    extend_chain(
        &target,
        &relationships,
        vec![target.clone()],
        Vec::new(),
        &mut proof_chains,
    );
    if proof_chains.len() >= 16 {
        unknowns.push("proof chain listing capped at 16 chains of at most 4 links".into());
    }
    Ok(WhyResult {
        proof_chains,
        target: target.to_string(),
        label: node.label().to_string(),
        relationships,
        unknowns,
    })
}

/// Every scan re-observes unchanged facts; show each distinct fact once with its
/// repeat count and first/latest time instead of one identical line per scan.
fn collapse_repeated(sorted: Vec<WhyObservation>) -> Vec<WhyObservation> {
    let mut collapsed: Vec<WhyObservation> = Vec::new();
    for observation in sorted {
        let existing = collapsed.iter_mut().find(|known| {
            known.source == observation.source
                && known.kind == observation.kind
                && known.raw_ref == observation.raw_ref
                && known.facts == observation.facts
        });
        match existing {
            Some(known) => {
                known.repeat_count += 1;
                known.id = observation.id;
                known.timestamp_ns = observation.timestamp_ns;
            }
            None => collapsed.push(observation),
        }
    }
    collapsed.sort_by(|a, b| {
        a.timestamp_ns
            .cmp(&b.timestamp_ns)
            .then_with(|| a.id.cmp(&b.id))
    });
    collapsed
}

fn proof_link(kind: &str) -> bool {
    matches!(
        EdgeKind::from_str(kind),
        Ok(EdgeKind::ConfiguredBy
            | EdgeKind::References
            | EdgeKind::ListensOn
            | EdgeKind::Owns
            | EdgeKind::ConnectsTo
            | EdgeKind::ProxiesTo
            | EdgeKind::DependsOn
            | EdgeKind::LoadsLibrary
            | EdgeKind::InstalledBy
            | EdgeKind::RunsImage
            | EdgeKind::MapsPort
            | EdgeKind::Selects
            | EdgeKind::RoutesTo)
    )
}

fn proof_rows(
    store: &Store,
    target: &NodeId,
) -> Result<(Vec<twin_store::EdgeRow>, bool), AppError> {
    let mut rows = BTreeMap::new();
    let mut visited = BTreeSet::from([target.to_string()]);
    let mut frontier = visited.clone();
    let mut capped = false;
    for depth in 0..4 {
        let mut next = BTreeSet::new();
        for node in frontier {
            let mut adjacent = store
                .list_active_edges_from(&node)
                .map_err(GraphError::Store)?;
            adjacent.extend(
                store
                    .list_active_edges_to(&node)
                    .map_err(GraphError::Store)?,
            );
            adjacent.sort_by(|a, b| a.id.cmp(&b.id));
            for row in adjacent {
                if depth > 0 && !proof_link(&row.kind) {
                    continue;
                }
                if rows.contains_key(&row.id) {
                    continue;
                }
                if rows.len() == 64 {
                    capped = true;
                    continue;
                }
                let peer = if row.from_node_id == node {
                    &row.to_node_id
                } else {
                    &row.from_node_id
                };
                if !visited.contains(peer) && proof_link(&row.kind) {
                    if visited.len() < 32 {
                        visited.insert(peer.clone());
                        next.insert(peer.clone());
                    } else {
                        capped = true;
                        continue;
                    }
                }
                rows.insert(row.id.clone(), row);
            }
        }
        frontier = next;
    }
    Ok((rows.into_values().collect(), capped))
}

fn extend_chain(
    current: &NodeId,
    relationships: &[WhyRelationship],
    nodes: Vec<NodeId>,
    links: Vec<usize>,
    chains: &mut Vec<WhyProofChain>,
) {
    if chains.len() >= 16 {
        return;
    }
    let mut extended = false;
    if links.len() < 4 {
        for (index, edge) in relationships.iter().enumerate() {
            if !proof_link(&edge.kind) {
                continue;
            }
            let peer = if edge.from == current.as_str() {
                &edge.to
            } else if edge.to == current.as_str() {
                &edge.from
            } else {
                continue;
            };
            let Ok(peer) = NodeId::from_str(peer) else {
                continue;
            };
            if nodes.contains(&peer) {
                continue;
            }
            extended = true;
            let mut next_nodes = nodes.clone();
            next_nodes.push(peer.clone());
            let mut next_links = links.clone();
            next_links.push(index);
            extend_chain(&peer, relationships, next_nodes, next_links, chains);
            if chains.len() >= 16 {
                break;
            }
        }
    }
    if !extended && links.len() >= 2 && chains.len() < 16 {
        let steps = links
            .iter()
            .filter_map(|index| relationships.get(*index))
            .collect::<Vec<_>>();
        chains.push(WhyProofChain {
            nodes,
            relationship_ids: steps.iter().map(|step| step.id.clone()).collect(),
            class: if steps.iter().all(|step| step.class == "observed") {
                "observed"
            } else {
                "inferred"
            }
            .into(),
        });
    }
}

pub fn request(
    raw: &str,
    config_override: Option<std::path::PathBuf>,
) -> Result<WhyRequest, AppError> {
    if raw.contains(':') {
        let target = NodeId::from_str(raw).map_err(|source| AppError::InvalidGraphTarget {
            value: raw.into(),
            source,
        })?;
        Ok(WhyRequest {
            config_override,
            target: Some(target),
            target_query: None,
        })
    } else {
        Ok(WhyRequest {
            config_override,
            target: None,
            target_query: Some(raw.into()),
        })
    }
}

fn safe_facts(metadata_json: &str) -> Vec<String> {
    let Ok(metadata) = serde_json::from_str::<serde_json::Value>(metadata_json) else {
        return Vec::new();
    };
    let mut facts = Vec::new();
    for key in [
        "pid",
        "ppid",
        "inode",
        "fd",
        "unit",
        "dep_key",
        "dependency",
        "path",
        "file",
        "line",
        "local_address",
        "local_port",
        "remote_address",
        "remote_ip",
        "local_ip",
        "remote_port",
        "protocol",
        "target",
        "proxy_scheme",
        "cgroup",
        "active_state",
        "load_state",
        "sub_state",
        "start_ticks",
        "network_namespace",
    ] {
        if let Some(value) = metadata.get(key) {
            if (key != "line" || value.is_number())
                && (value.is_string() || value.is_number() || value.is_boolean())
            {
                facts.push(format!(
                    "{key}={}",
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                ));
            }
        }
    }
    facts
}
