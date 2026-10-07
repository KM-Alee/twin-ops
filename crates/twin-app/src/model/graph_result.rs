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
pub struct GraphOwnedNode {
    pub id: String,
    pub label: String,
    pub edge_class: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    pub observation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeDependency {
    pub from_id: String,
    pub to_id: String,
    pub relationship: String,
    pub evidence_label: String,
    pub evidence_score: u8,
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphEvidenceLine {
    pub source: String,
    pub statement: String,
    pub strength: String,
    pub relationship: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphServiceResult {
    pub service: GraphNodeSummary,
    pub owned_processes: Vec<GraphOwnedNode>,
    pub owned_cgroups: Vec<GraphOwnedNode>,
    pub listening_ports: Vec<GraphOwnedNode>,
    pub listening_unix: Vec<GraphOwnedNode>,
    pub connected_ports: Vec<GraphOwnedNode>,
    pub connected_unix: Vec<GraphOwnedNode>,
    pub dependencies: Vec<GraphOwnedNode>,
    pub dependents: Vec<GraphOwnedNode>,
    pub socket_activation: Vec<GraphOwnedNode>,
    pub configured_dependents: Vec<GraphOwnedNode>,
    pub configured_files: Vec<GraphOwnedNode>,
    pub evidence: Vec<GraphEvidenceLine>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_dependencies: Vec<RuntimeDependency>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphFileResult {
    pub file: GraphNodeSummary,
    pub configures: Vec<GraphOwnedNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub references: Vec<GraphOwnedNode>,
    pub evidence: Vec<GraphEvidenceLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphPortResult {
    pub port: GraphNodeSummary,
    pub process_listeners: Vec<GraphOwnedNode>,
    pub service_listeners: Vec<GraphOwnedNode>,
    pub process_callers: Vec<GraphOwnedNode>,
    pub service_callers: Vec<GraphOwnedNode>,
    pub evidence: Vec<GraphEvidenceLine>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GraphUnixSocketResult {
    pub unix_socket: GraphNodeSummary,
    pub process_listeners: Vec<GraphOwnedNode>,
    pub service_listeners: Vec<GraphOwnedNode>,
    pub process_callers: Vec<GraphOwnedNode>,
    pub service_callers: Vec<GraphOwnedNode>,
    pub evidence: Vec<GraphEvidenceLine>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(untagged)]
pub enum GraphResult {
    List(GraphListResult),
    Node(GraphNodeResult),
    Service(GraphServiceResult),
    Port(GraphPortResult),
    UnixSocket(GraphUnixSocketResult),
    File(GraphFileResult),
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

    pub fn service(result: GraphServiceResult) -> Self {
        Self::Service(result)
    }

    pub fn port(
        port: GraphNodeSummary,
        process_listeners: Vec<GraphOwnedNode>,
        service_listeners: Vec<GraphOwnedNode>,
        process_callers: Vec<GraphOwnedNode>,
        service_callers: Vec<GraphOwnedNode>,
        evidence: Vec<GraphEvidenceLine>,
    ) -> Self {
        Self::Port(GraphPortResult {
            port,
            process_listeners,
            service_listeners,
            process_callers,
            service_callers,
            evidence,
        })
    }

    pub fn file(
        file: GraphNodeSummary,
        configures: Vec<GraphOwnedNode>,
        references: Vec<GraphOwnedNode>,
        evidence: Vec<GraphEvidenceLine>,
    ) -> Self {
        Self::File(GraphFileResult {
            file,
            configures,
            references,
            evidence,
        })
    }

    pub fn unix_socket(
        unix_socket: GraphNodeSummary,
        process_listeners: Vec<GraphOwnedNode>,
        service_listeners: Vec<GraphOwnedNode>,
        process_callers: Vec<GraphOwnedNode>,
        service_callers: Vec<GraphOwnedNode>,
        evidence: Vec<GraphEvidenceLine>,
    ) -> Self {
        Self::UnixSocket(GraphUnixSocketResult {
            unix_socket,
            process_listeners,
            service_listeners,
            process_callers,
            service_callers,
            evidence,
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
