use twin_core::{EdgeClass, EdgeId, EdgeKind, EvidenceRecency, NodeId};

#[derive(Debug, Clone)]
pub struct EmulationNode {
    pub id: NodeId,
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct EmulationPathStep {
    pub from_id: NodeId,
    pub from_label: String,
    pub edge_kind: EdgeKind,
    pub edge_class: EdgeClass,
    pub to_id: NodeId,
    pub to_label: String,
    pub edge_id: EdgeId,
}

#[derive(Debug, Clone)]
pub struct EmulationEvidenceLine {
    pub source: String,
    pub statement: String,
    pub relationship: String,
    pub strength_score: u8,
}

#[derive(Debug, Clone)]
pub struct EmulationDependent {
    pub id: NodeId,
    pub label: String,
    pub edge_kind: EdgeKind,
    pub edge_class: EdgeClass,
    pub reason: String,
    pub path: Vec<EmulationPathStep>,
    pub evidence: Vec<EmulationEvidenceLine>,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct EmulationUnknown {
    pub kind: String,
    pub detail: String,
    pub source: Option<String>,
    pub weakens_evidence: bool,
}

#[derive(Debug, Clone)]
pub struct EmulationImpactPath {
    pub terminal_id: NodeId,
    pub terminal_label: String,
    pub depth: usize,
    pub steps: Vec<EmulationPathStep>,
    pub evidence: Vec<EmulationEvidenceLine>,
    pub is_cycle_capped: bool,
    pub is_depth_capped: bool,
    pub cycle_note: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct RestartPathScoringInput {
    pub transitive_runtime_count: usize,
    pub public_exposure_label: Option<String>,
    pub criticality_hints: Vec<String>,
}

fn default_max_depth() -> usize {
    4
}

#[derive(Debug, Clone)]
pub struct RestartServiceInput {
    pub target: EmulationNode,
    pub unavailable_nodes: Vec<EmulationNode>,
    pub runtime_dependents: Vec<EmulationDependent>,
    pub configured_dependents: Vec<EmulationDependent>,
    pub unknowns: Vec<EmulationUnknown>,
    pub impact_paths: Vec<EmulationImpactPath>,
    pub paths_requested: bool,
    pub max_depth: usize,
    pub path_scoring: Option<RestartPathScoringInput>,
    pub permission_gaps: u32,
    pub dropped_ebpf: bool,
    pub recency: EvidenceRecency,
}

impl Default for RestartServiceInput {
    fn default() -> Self {
        Self {
            target: EmulationNode {
                id: NodeId::service("unknown.service"),
                label: "unknown.service".to_string(),
            },
            unavailable_nodes: Vec::new(),
            runtime_dependents: Vec::new(),
            configured_dependents: Vec::new(),
            unknowns: Vec::new(),
            impact_paths: Vec::new(),
            paths_requested: false,
            max_depth: default_max_depth(),
            path_scoring: None,
            permission_gaps: 0,
            dropped_ebpf: false,
            recency: EvidenceRecency::Unknown,
        }
    }
}

#[derive(Debug, Clone)]
pub struct EmulationConfiguredService {
    pub id: NodeId,
    pub label: String,
    pub file_id: NodeId,
    pub evidence: Vec<EmulationEvidenceLine>,
}

#[derive(Debug, Clone)]
pub struct DeleteFileInput {
    pub target: EmulationNode,
    pub configured_services: Vec<EmulationConfiguredService>,
    pub evidence: Vec<EmulationEvidenceLine>,
    pub unknowns: Vec<EmulationUnknown>,
    pub file_in_graph: bool,
    pub file_exists: bool,
    pub permission_gaps: u32,
    pub recency: EvidenceRecency,
}
