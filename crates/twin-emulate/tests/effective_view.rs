use twin_core::{NodeId, NodeState};
use twin_emulate::{
    effective_view::EffectiveGraphView, EmulationAction, EmulationNode, GraphOverlay, NodeOverlay,
    OverlayNodeState,
};

#[test]
fn effective_view_reads_overlay_before_base() {
    let target = EmulationNode {
        id: NodeId::service("postgresql.service"),
        label: "postgresql.service".to_string(),
    };
    let overlay = GraphOverlay {
        action: EmulationAction::RestartService {
            target: target.id.clone(),
        },
        node_overrides: vec![NodeOverlay {
            node_id: target.id.clone(),
            state: OverlayNodeState::TemporarilyUnavailable,
            reason: "restart".to_string(),
        }],
        interrupted_relationships: vec![],
    };
    let base_nodes = [target.clone()];
    let view = EffectiveGraphView::new(&base_nodes, NodeState::Active, &overlay);
    assert!(view.is_temporarily_unavailable(&target.id));
    assert_eq!(view.node_state(&target.id), NodeState::Stale);
    let other = NodeId::service("django.service");
    assert!(!view.is_temporarily_unavailable(&other));
    assert_eq!(view.node_state(&other), NodeState::Active);
}
