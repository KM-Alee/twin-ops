use twin_core::{EdgeId, EdgeKind, NodeId};

use crate::action::EmulationAction;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayNodeState {
    TemporarilyUnavailable,
}

#[derive(Debug, Clone)]
pub struct NodeOverlay {
    pub node_id: NodeId,
    pub state: OverlayNodeState,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct InterruptedRelationship {
    pub edge_id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct GraphOverlay {
    pub action: EmulationAction,
    pub node_overrides: Vec<NodeOverlay>,
    pub interrupted_relationships: Vec<InterruptedRelationship>,
}
