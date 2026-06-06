use std::collections::HashMap;

use twin_core::{EdgeId, NodeId, NodeState};

use crate::input::EmulationNode;
use crate::overlay::{GraphOverlay, InterruptedRelationship, OverlayNodeState};

pub struct EffectiveGraphView<'a> {
    base_nodes: &'a [EmulationNode],
    base_state: NodeState,
    overlay: &'a GraphOverlay,
    node_override_map: HashMap<&'a str, OverlayNodeState>,
    interrupted_map: HashMap<&'a str, &'a InterruptedRelationship>,
}

impl<'a> EffectiveGraphView<'a> {
    pub fn new(
        base_nodes: &'a [EmulationNode],
        base_state: NodeState,
        overlay: &'a GraphOverlay,
    ) -> Self {
        let mut node_override_map = HashMap::new();
        for node in &overlay.node_overrides {
            node_override_map.insert(node.node_id.as_str(), node.state);
        }
        let mut interrupted_map = HashMap::new();
        for rel in &overlay.interrupted_relationships {
            interrupted_map.insert(rel.edge_id.as_str(), rel);
        }
        Self {
            base_nodes,
            base_state,
            overlay,
            node_override_map,
            interrupted_map,
        }
    }

    pub fn node_state(&self, node_id: &NodeId) -> NodeState {
        if self.is_temporarily_unavailable(node_id) {
            return NodeState::Stale;
        }
        self.base_state
    }

    pub fn is_temporarily_unavailable(&self, node_id: &NodeId) -> bool {
        self.node_override_map
            .get(node_id.as_str())
            .copied()
            .is_some_and(|s| s == OverlayNodeState::TemporarilyUnavailable)
    }

    pub fn interrupted_edge(&self, edge_id: &EdgeId) -> Option<&InterruptedRelationship> {
        self.interrupted_map.get(edge_id.as_str()).copied()
    }

    pub fn overlay(&self) -> &GraphOverlay {
        self.overlay
    }

    pub fn base_nodes(&self) -> &[EmulationNode] {
        self.base_nodes
    }
}
