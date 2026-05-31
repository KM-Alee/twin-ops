use std::str::FromStr;

use twin_core::{
    EdgeClass, EdgeId, EdgeKind, EdgeState, GraphEdge, GraphEdgeParts, GraphMetadata, GraphNode,
    GraphNodeParts, NodeId, NodeKind, NodeState, TimestampNs,
};

use crate::error::StoreError;
use crate::repo::{EdgeRow, NodeRow};
use crate::store::Store;

impl From<&GraphNode> for NodeRow {
    fn from(node: &GraphNode) -> Self {
        NodeRow {
            id: node.id().to_string(),
            kind: node.kind().to_string(),
            label: node.label().to_string(),
            state: node.state().to_string(),
            first_seen_ns: node.first_seen().as_i64(),
            last_seen_ns: node.last_seen().as_i64(),
            valid_from_ns: node.valid_from().as_i64(),
            valid_to_ns: node.valid_to().map(TimestampNs::as_i64),
            metadata_json: node.metadata().as_str().to_string(),
        }
    }
}

impl TryFrom<&NodeRow> for GraphNode {
    type Error = StoreError;

    fn try_from(row: &NodeRow) -> Result<Self, StoreError> {
        let id = parse_field("node id", &row.id, NodeId::from_str)?;
        let kind = parse_field("kind", &row.kind, NodeKind::from_str)?;
        let state = parse_field("state", &row.state, NodeState::from_str)?;
        Ok(GraphNode::from_parts(GraphNodeParts {
            id,
            kind,
            label: row.label.clone(),
            state,
            first_seen: TimestampNs::new(row.first_seen_ns),
            last_seen: TimestampNs::new(row.last_seen_ns),
            valid_from: TimestampNs::new(row.valid_from_ns),
            valid_to: row.valid_to_ns.map(TimestampNs::new),
            metadata: GraphMetadata::from_json(row.metadata_json.clone()),
        }))
    }
}

impl From<&GraphEdge> for EdgeRow {
    fn from(edge: &GraphEdge) -> Self {
        EdgeRow {
            id: edge.id().to_string(),
            from_node_id: edge.from().to_string(),
            to_node_id: edge.to().to_string(),
            kind: edge.kind().to_string(),
            class: edge.class().to_string(),
            state: edge.state().to_string(),
            evidence_score: 100,
            evidence_label: "strong".to_string(),
            evidence_count: edge.evidence_count() as i64,
            first_seen_ns: edge.first_seen().as_i64(),
            last_seen_ns: edge.last_seen().as_i64(),
            metadata_json: edge.metadata().as_str().to_string(),
        }
    }
}

impl TryFrom<&EdgeRow> for GraphEdge {
    type Error = StoreError;

    fn try_from(row: &EdgeRow) -> Result<Self, Self::Error> {
        let from = parse_field("from_node_id", &row.from_node_id, NodeId::from_str)?;
        let kind = parse_field("kind", &row.kind, EdgeKind::from_str)?;
        let to = parse_field("to_node_id", &row.to_node_id, NodeId::from_str)?;
        let id = EdgeId::new(&from, kind, &to);
        if id.as_str() != row.id {
            return Err(StoreError::Decode {
                detail: format!("edge id mismatch: stored `{}`", row.id),
            });
        }
        let class = parse_field("class", &row.class, EdgeClass::from_str)?;
        let state = parse_field("state", &row.state, EdgeState::from_str)?;
        Ok(GraphEdge::from_stored(GraphEdgeParts {
            from,
            to,
            kind,
            class,
            state,
            evidence_count: row.evidence_count.max(0) as u32,
            first_seen: TimestampNs::new(row.first_seen_ns),
            last_seen: TimestampNs::new(row.last_seen_ns),
            metadata: GraphMetadata::from_json(row.metadata_json.clone()),
        }))
    }
}

fn parse_field<T, E, F>(field: &str, value: &str, parse: F) -> Result<T, StoreError>
where
    E: std::fmt::Display,
    F: FnOnce(&str) -> Result<T, E>,
{
    parse(value).map_err(|e| StoreError::Decode {
        detail: format!("invalid {field} `{value}`: {e}"),
    })
}

impl Store {
    pub fn upsert_node_typed(&mut self, node: &GraphNode) -> Result<(), StoreError> {
        self.upsert_node(&NodeRow::from(node))
    }

    pub fn upsert_edge_typed(&mut self, edge: &GraphEdge) -> Result<(), StoreError> {
        self.upsert_edge(&EdgeRow::from(edge))
    }

    pub fn get_node_typed(&self, id: &NodeId) -> Result<Option<GraphNode>, StoreError> {
        match self.get_node(id.as_str())? {
            Some(row) => Ok(Some(GraphNode::try_from(&row)?)),
            None => Ok(None),
        }
    }

    pub fn list_nodes_by_kind_typed(&self, kind: NodeKind) -> Result<Vec<GraphNode>, StoreError> {
        let rows = self.list_nodes_by_kind(&kind.to_string())?;
        rows.iter().map(GraphNode::try_from).collect()
    }
}
