use std::collections::{HashSet, VecDeque};
use std::convert::TryFrom;

use twin_core::{EdgeClass, EdgeKind, EdgeState, GraphEdge, NodeId, NodeKind};
use twin_store::Store;

use crate::commands::evidence::classify_dependent_impact_kind;
use crate::commands::impact::{
    collect_edge_evidence, is_active_edge, missing_evidence_unknown, node_summary,
};
use crate::error::{AppError, ImpactError};
use crate::model::{ImpactNodeSummary, ImpactPath, ImpactPathStep, ImpactUnknown};

pub(crate) struct ImpactPathAnalysis {
    pub paths: Vec<ImpactPath>,
    pub unknowns: Vec<ImpactUnknown>,
    pub transitive_runtime_count: usize,
    pub public_exposure_label: Option<String>,
    pub criticality_hints: Vec<String>,
}

pub(crate) fn load_service_impact_paths(
    store: &Store,
    target: &NodeId,
    target_label: &str,
    max_depth: usize,
) -> Result<ImpactPathAnalysis, AppError> {
    let mut collector = PathCollector::new(store, target, max_depth);
    collector.walk(
        target,
        target_label,
        Vec::new(),
        HashSet::from([target.clone()]),
        0,
    )?;
    collector.sort_and_dedupe();
    let transitive_runtime_count = collector.transitive_runtime_count();
    let public_exposure_label = collector.public_exposure_label();
    let mut criticality_hints = service_criticality_hints(target_label);
    for path in &collector.paths {
        for hint in service_criticality_hints(&path.terminal.label) {
            if !criticality_hints.contains(&hint) {
                criticality_hints.push(hint);
            }
        }
    }
    Ok(ImpactPathAnalysis {
        paths: collector.paths,
        unknowns: collector.unknowns,
        transitive_runtime_count,
        public_exposure_label,
        criticality_hints,
    })
}

pub(crate) fn port_impact_paths_from_dependents(
    direct_dependents: &[crate::model::ImpactDependent],
) -> Vec<ImpactPath> {
    let mut paths = Vec::new();
    for dependent in direct_dependents {
        if dependent.path.is_empty() {
            continue;
        }
        paths.push(ImpactPath {
            terminal: ImpactNodeSummary {
                id: dependent.id.clone(),
                label: dependent.label.clone(),
            },
            depth: dependent.path.len(),
            steps: dependent.path.clone(),
            evidence: dependent.evidence.clone(),
            is_cycle_capped: false,
            is_depth_capped: false,
            cycle_note: None,
        });
    }
    paths.sort_by(path_sort_key);
    paths
}

struct PathCollector<'a> {
    store: &'a Store,
    target: &'a NodeId,
    max_depth: usize,
    paths: Vec<ImpactPath>,
    unknowns: Vec<ImpactUnknown>,
    seen_evidence: HashSet<String>,
    seen_chains: HashSet<String>,
}

impl<'a> PathCollector<'a> {
    fn new(store: &'a Store, target: &'a NodeId, max_depth: usize) -> Self {
        Self {
            store,
            target,
            max_depth,
            paths: Vec::new(),
            unknowns: Vec::new(),
            seen_evidence: HashSet::new(),
            seen_chains: HashSet::new(),
        }
    }

    fn walk(
        &mut self,
        current: &NodeId,
        current_label: &str,
        steps: Vec<ImpactPathStep>,
        visited: HashSet<NodeId>,
        depth: usize,
    ) -> Result<(), AppError> {
        let incoming = self.runtime_incoming_edges(current)?;
        if depth >= self.max_depth {
            if !incoming.is_empty() && !steps.is_empty() {
                self.emit_path(&steps, false, true, None)?;
            }
            return Ok(());
        }
        if incoming.is_empty() {
            if !steps.is_empty() {
                self.emit_path(&steps, false, false, None)?;
            }
            return Ok(());
        }
        for edge in incoming {
            let dependent_id = edge.from().clone();
            let dependent_node = self
                .store
                .get_node_typed(&dependent_id)
                .map_err(ImpactError::Store)?
                .ok_or_else(|| ImpactError::NodeNotFound {
                    id: dependent_id.clone(),
                })?;
            let step = self.build_step(
                &dependent_id,
                dependent_node.label(),
                current,
                current_label,
                &edge,
            )?;
            if visited.contains(&dependent_id) {
                let mut cycle_steps = vec![step];
                cycle_steps.extend(steps.clone());
                let note = format!(
                    "cycle capped at {}; path already contains this node",
                    dependent_id
                );
                self.emit_path(&cycle_steps, true, false, Some(note))?;
                continue;
            }
            let mut next_steps = vec![step];
            next_steps.extend(steps.clone());
            let mut next_visited = visited.clone();
            next_visited.insert(dependent_id.clone());
            self.walk(
                &dependent_id,
                dependent_node.label(),
                next_steps,
                next_visited,
                depth + 1,
            )?;
        }
        Ok(())
    }

    fn runtime_incoming_edges(&self, current: &NodeId) -> Result<Vec<GraphEdge>, AppError> {
        let mut edges = Vec::new();
        for row in self
            .store
            .list_active_edges_to(current.as_str())
            .map_err(ImpactError::Store)?
        {
            let edge = GraphEdge::try_from(&row).map_err(ImpactError::Store)?;
            if !is_active_edge(&edge) || edge.kind() != EdgeKind::DependsOn {
                continue;
            }
            let dependent_node = self
                .store
                .get_node_typed(edge.from())
                .map_err(ImpactError::Store)?
                .ok_or_else(|| ImpactError::NodeNotFound {
                    id: edge.from().clone(),
                })?;
            let impact_kind =
                classify_dependent_impact_kind(&edge, dependent_node.metadata().as_str());
            if impact_kind.is_runtime() {
                edges.push(edge);
            }
        }
        edges.sort_by(|a, b| {
            a.from()
                .as_str()
                .cmp(b.from().as_str())
                .then_with(|| a.id().as_str().cmp(b.id().as_str()))
        });
        Ok(edges)
    }

    fn build_step(
        &mut self,
        from_id: &NodeId,
        from_label: &str,
        to_id: &NodeId,
        to_label: &str,
        edge: &GraphEdge,
    ) -> Result<ImpactPathStep, AppError> {
        let obs_links = self
            .store
            .list_observations_for_edge(edge.id().as_str())
            .map_err(ImpactError::Store)?;
        let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
        if observation_ids.is_empty() {
            self.unknowns
                .push(missing_evidence_unknown(edge.id().as_str()));
        }
        Ok(ImpactPathStep {
            from: node_summary(from_id, from_label),
            edge_kind: edge.kind().to_string(),
            edge_class: edge.class().to_string(),
            to: node_summary(to_id, to_label),
            edge_id: edge.id().to_string(),
        })
    }

    fn emit_path(
        &mut self,
        steps: &[ImpactPathStep],
        is_cycle_capped: bool,
        is_depth_capped: bool,
        cycle_note: Option<String>,
    ) -> Result<(), AppError> {
        if steps.is_empty() {
            return Ok(());
        }
        let last_to_id = steps
            .last()
            .map(|step| step.to.id.clone())
            .unwrap_or_default();
        let chain_key = steps
            .iter()
            .map(|s| s.from.id.clone())
            .chain(std::iter::once(last_to_id))
            .collect::<Vec<_>>()
            .join("->");
        if !self.seen_chains.insert(chain_key) {
            return Ok(());
        }
        let mut path_evidence = Vec::new();
        for step in steps {
            let edge = self
                .store
                .list_active_edges_to(step.to.id.as_str())
                .map_err(ImpactError::Store)?
                .into_iter()
                .find(|row| row.id == step.edge_id)
                .and_then(|row| GraphEdge::try_from(&row).ok());
            let Some(edge) = edge else {
                continue;
            };
            let obs_links = self
                .store
                .list_observations_for_edge(step.edge_id.as_str())
                .map_err(ImpactError::Store)?;
            let observation_ids: Vec<String> = obs_links.iter().map(|(id, _)| id.clone()).collect();
            let relationship = if edge.class() == EdgeClass::Observed {
                "Configured: systemd unit file dependency"
            } else {
                "Inferred: service dependency inferred from active connection and listener match"
            };
            collect_edge_evidence(
                self.store,
                &observation_ids,
                &mut path_evidence,
                &mut self.seen_evidence,
                relationship,
            );
        }
        let terminal = steps[0].from.clone();
        self.paths.push(ImpactPath {
            terminal,
            depth: steps.len(),
            steps: steps.to_vec(),
            evidence: path_evidence,
            is_cycle_capped,
            is_depth_capped,
            cycle_note,
        });
        Ok(())
    }

    fn sort_and_dedupe(&mut self) {
        self.paths.sort_by(path_sort_key);
        self.unknowns.sort_by(|a, b| a.kind.cmp(&b.kind));
    }

    fn transitive_runtime_count(&self) -> usize {
        self.paths
            .iter()
            .filter(|p| p.depth >= 2)
            .map(|p| p.terminal.id.as_str())
            .collect::<HashSet<_>>()
            .len()
    }

    fn public_exposure_label(&self) -> Option<String> {
        for path in &self.paths {
            let mut nodes: VecDeque<&ImpactNodeSummary> = VecDeque::new();
            nodes.push_back(&path.terminal);
            for step in &path.steps {
                nodes.push_back(&step.to);
            }
            for node in nodes {
                if let Some(label) = service_has_public_listener(self.store, &node.id) {
                    return Some(label);
                }
            }
        }
        if let Some(label) = service_has_public_listener(self.store, self.target.as_str()) {
            return Some(label);
        }
        None
    }
}

fn path_sort_key(a: &ImpactPath, b: &ImpactPath) -> std::cmp::Ordering {
    a.terminal
        .id
        .cmp(&b.terminal.id)
        .then_with(|| a.depth.cmp(&b.depth))
        .then_with(|| {
            let a_edges: String = a.steps.iter().map(|s| s.edge_id.as_str()).collect();
            let b_edges: String = b.steps.iter().map(|s| s.edge_id.as_str()).collect();
            a_edges.cmp(&b_edges)
        })
}

const CRITICALITY_HINTS: &[&str] = &[
    "postgres",
    "mysql",
    "mariadb",
    "redis",
    "rabbitmq",
    "nats",
    "kafka",
    "etcd",
    "containerd",
];

pub(crate) fn service_criticality_hints(label: &str) -> Vec<String> {
    let lower = label.to_lowercase();
    CRITICALITY_HINTS
        .iter()
        .filter(|hint| lower.contains(**hint))
        .map(|hint| format!("service criticality hint: {hint}"))
        .collect()
}

fn service_has_public_listener(store: &Store, service_id: &str) -> Option<String> {
    let service = NodeId::from_str(service_id).ok()?;
    if service.kind() != Some(NodeKind::Service) {
        return None;
    }
    for row in store.list_active_edges_from(service_id).ok()? {
        let edge = GraphEdge::try_from(&row).ok()?;
        if edge.state() != EdgeState::Active || edge.kind() != EdgeKind::ListensOn {
            continue;
        }
        let port_id = edge.to();
        if is_non_loopback_port(port_id.as_str()) {
            let node = store.get_node_typed(port_id).ok()??;
            return Some(node.label().to_string());
        }
    }
    None
}

fn is_non_loopback_port(port_id: &str) -> bool {
    let Some(rest) = port_id.strip_prefix("port:tcp:") else {
        return false;
    };
    let (ip, _) = rest.rsplit_once(':').unwrap_or((rest, ""));
    ip == "0.0.0.0" || ip == "::" || (!ip.starts_with("127.") && ip != "::1")
}

use std::str::FromStr;
