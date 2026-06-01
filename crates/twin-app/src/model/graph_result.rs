use serde::Serialize;
use twin_core::{EdgeKind, NodeId, NodeKind};

#[derive(Debug, Clone, Serialize)]
pub struct GraphNodeSummary {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEdgeSummary {
    pub id: String,
    pub kind: String,
    pub peer_id: String,
    pub peer_label: String,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphParentEdge {
    pub parent_id: String,
    pub child_id: String,
    pub child_label: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphListResult {
    pub kind: NodeKind,
    pub nodes: Vec<GraphNodeSummary>,
    pub parent_edges: Vec<GraphParentEdge>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphNodeResult {
    pub node: GraphNodeSummary,
    pub outgoing: Vec<GraphEdgeSummary>,
    pub incoming: Vec<GraphEdgeSummary>,
    pub evidence_refs: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum GraphResult {
    List(GraphListResult),
    Node(GraphNodeResult),
}

impl GraphResult {
    pub fn list(
        kind: NodeKind,
        nodes: Vec<GraphNodeSummary>,
        parent_edges: Vec<GraphParentEdge>,
    ) -> Self {
        Self::List(GraphListResult {
            kind,
            nodes,
            parent_edges,
        })
    }

    pub fn node(
        node: GraphNodeSummary,
        outgoing: Vec<GraphEdgeSummary>,
        incoming: Vec<GraphEdgeSummary>,
        evidence_refs: Vec<String>,
    ) -> Self {
        Self::Node(GraphNodeResult {
            node,
            outgoing,
            incoming,
            evidence_refs,
        })
    }
}

pub fn edge_summary(
    edge_id: &str,
    kind: EdgeKind,
    peer_id: &NodeId,
    peer_label: &str,
    observation_ids: Vec<String>,
) -> GraphEdgeSummary {
    GraphEdgeSummary {
        id: edge_id.to_string(),
        kind: kind.to_string(),
        peer_id: peer_id.to_string(),
        peer_label: peer_label.to_string(),
        observation_ids,
    }
}

pub fn node_summary(id: &NodeId, label: &str) -> GraphNodeSummary {
    GraphNodeSummary {
        id: id.to_string(),
        label: label.to_string(),
    }
}
