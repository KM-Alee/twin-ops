use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DiffResult {
    pub left_ref: String,
    pub right_ref: String,
    pub nodes_added: Vec<DiffNode>,
    pub nodes_removed: Vec<DiffNode>,
    pub nodes_changed: Vec<DiffNodeChange>,
    pub edges_added: Vec<DiffEdge>,
    pub edges_removed: Vec<DiffEdge>,
    pub edges_changed: Vec<DiffEdgeChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffNode {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffEdge {
    pub id: String,
    pub from_node_id: String,
    pub to_node_id: String,
    pub kind: String,
    pub class: String,
    pub state: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffNodeChange {
    pub id: String,
    pub label: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiffEdgeChange {
    pub id: String,
    pub summary: String,
}
