mod support;

use std::convert::TryFrom;
use std::str::FromStr;

use twin_core::{
    EdgeClass, EdgeId, EdgeKind, EdgeState, GraphEdge, GraphNode, NodeId, NodeKind, NodeState,
    TimestampNs,
};

use support::{blank_store, TS};

#[test]
fn graph_node_round_trips_through_store() {
    let mut store = blank_store();
    let id = NodeId::process(42);
    let node = GraphNode::process(42, "nginx", TimestampNs::new(TS), None);
    store.upsert_node_typed(&node).expect("upsert");
    let got = store.get_node_typed(&id).expect("get").expect("row");
    assert_eq!(got.id(), &id);
    assert_eq!(got.kind(), NodeKind::Process);
    assert_eq!(got.label(), "nginx");
    assert_eq!(got.state(), NodeState::Active);
}

#[test]
fn graph_edge_round_trips_through_store() {
    let mut store = blank_store();
    let parent = NodeId::process(1);
    let child = NodeId::process(42);
    store
        .upsert_node_typed(&GraphNode::process(
            1,
            "systemd",
            TimestampNs::new(TS),
            None,
        ))
        .expect("parent");
    store
        .upsert_node_typed(&GraphNode::process(42, "nginx", TimestampNs::new(TS), None))
        .expect("child");
    let edge = GraphEdge::observed_parent(&parent, &child, TimestampNs::new(TS), None);
    store.upsert_edge_typed(&edge).expect("edge");
    let edge_id = EdgeId::new(&parent, EdgeKind::ParentOf, &child);
    let row = store.get_edge(edge_id.as_str()).expect("get").expect("row");
    let got = GraphEdge::try_from(&row).expect("typed");
    assert_eq!(got.kind(), EdgeKind::ParentOf);
    assert_eq!(got.class(), EdgeClass::Observed);
    assert_eq!(got.state(), EdgeState::Active);
    assert_eq!(got.evidence_count(), 1);
}

#[test]
fn upsert_preserves_first_seen_and_updates_last_seen() {
    let mut store = blank_store();
    let id = NodeId::process(7);
    let first = GraphNode::process(7, "a", TimestampNs::new(TS), None);
    store.upsert_node_typed(&first).expect("first");
    let existing = store.get_node_typed(&id).expect("get");
    let second = GraphNode::process(7, "b", TimestampNs::new(TS + 50), existing.as_ref());
    store.upsert_node_typed(&second).expect("second");
    let got = store.get_node_typed(&id).expect("get").expect("node");
    assert_eq!(got.first_seen().as_i64(), TS);
    assert_eq!(got.last_seen().as_i64(), TS + 50);
    assert_eq!(got.label(), "b");
}

#[test]
fn edge_observation_link_round_trips() {
    let mut store = blank_store();
    store
        .upsert_node(&support::node_row(
            "process:pid:1",
            "process",
            "systemd",
            TS,
        ))
        .expect("parent");
    store
        .upsert_node(&support::node_row("process:pid:42", "process", "nginx", TS))
        .expect("child");
    let edge_id = "process:pid:1|parent_of|process:pid:42";
    store
        .upsert_edge(&support::edge_row(
            edge_id,
            "process:pid:1",
            "process:pid:42",
            "parent_of",
            TS,
        ))
        .expect("edge");
    store
        .insert_observation(&support::observation_row(
            "obs-parent-1",
            "proc",
            "process_parent_seen",
            TS,
        ))
        .expect("obs");
    store
        .link_edge_observation(edge_id, "obs-parent-1", "support")
        .expect("link");
    let links = store.list_observations_for_edge(edge_id).expect("list");
    assert_eq!(
        links,
        vec![("obs-parent-1".to_string(), "support".to_string())]
    );
}
