use twin_core::{EdgeClass, EdgeId, EdgeKind, NodeId};

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
pub struct RestartServiceInput {
    pub target: EmulationNode,
    pub unavailable_nodes: Vec<EmulationNode>,
    pub runtime_dependents: Vec<EmulationDependent>,
    pub configured_dependents: Vec<EmulationDependent>,
    pub unknowns: Vec<EmulationUnknown>,
}
