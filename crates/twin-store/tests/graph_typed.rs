mod support;

use std::convert::TryFrom;

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
    let got = store
        .get_edge_typed(&edge_id)
        .expect("typed get")
        .expect("edge");
    assert_eq!(got, GraphEdge::try_from(&row).expect("typed"));
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

#[test]
fn batched_edge_links_handle_empty_missing_and_duplicate_ids_across_chunks() {
    let mut store = blank_store();
    let edge = GraphEdge::observed_parent(
        &NodeId::process(1),
        &NodeId::process(42),
        TimestampNs::new(TS),
        None,
    );
    for pid in [1, 42] {
        store
            .upsert_node_typed(&GraphNode::process(pid, "test", TimestampNs::new(TS), None))
            .unwrap();
    }
    store.upsert_edge_typed(&edge).unwrap();
    store
        .insert_observation(&support::observation_row(
            "a",
            "proc",
            "process_parent_seen",
            TS,
        ))
        .unwrap();
    store
        .insert_observation(&support::observation_row(
            "b",
            "proc",
            "process_parent_seen",
            TS,
        ))
        .unwrap();
    store
        .link_edge_observation(edge.id().as_str(), "b", "support")
        .unwrap();
    store
        .link_edge_observation(edge.id().as_str(), "a", "support")
        .unwrap();
    assert!(store.list_observations_for_edges(&[]).unwrap().is_empty());
    assert!(store
        .list_observations_for_edges(&["missing", "' OR 1=1 --"])
        .unwrap()
        .is_empty());
    let ids = vec![edge.id().as_str(); 1001];
    let batch = store.list_observations_for_edges(&ids).unwrap();
    assert_eq!(batch.len(), 1);
    assert_eq!(
        batch[edge.id().as_str()],
        store
            .list_observations_for_edge(edge.id().as_str())
            .unwrap()
    );
}

#[test]
fn typed_edge_lookup_distinguishes_missing_from_malformed_rows() {
    let mut store = blank_store();
    let from = NodeId::process(1);
    let to = NodeId::process(42);
    let id = EdgeId::new(&from, EdgeKind::ParentOf, &to);
    assert!(store.get_edge_typed(&id).unwrap().is_none());
    for pid in [1, 42] {
        store
            .upsert_node_typed(&GraphNode::process(pid, "test", TimestampNs::new(TS), None))
            .unwrap();
    }
    let mut row = support::edge_row(id.as_str(), from.as_str(), to.as_str(), "parent_of", TS);
    row.class = "invalid".into();
    store.upsert_edge(&row).unwrap();
    assert!(matches!(
        store.get_edge_typed(&id),
        Err(twin_store::StoreError::Decode { .. })
    ));
}

#[test]
fn batched_incoming_edges_match_single_reads_across_chunks_and_duplicate_ids() {
    let mut store = twin_store::Store::open_in_memory().expect("store");
    store.initialize().expect("initialize");
    for index in 0..503 {
        let id = format!("service:batch-{index}.service");
        store
            .upsert_node(&twin_store::NodeRow {
                id: id.clone(),
                kind: "service".into(),
                label: id.clone(),
                state: "active".into(),
                first_seen_ns: 1,
                last_seen_ns: 1,
                valid_from_ns: 1,
                valid_to_ns: None,
                metadata_json: "{}".into(),
            })
            .expect("node");
        store
            .upsert_edge(&twin_store::EdgeRow {
                id: format!("{id}|depends_on|{id}"),
                from_node_id: id.clone(),
                to_node_id: id,
                kind: "depends_on".into(),
                class: "observed".into(),
                state: if index == 0 { "stale" } else { "active" }.into(),
                evidence_score: 45,
                evidence_label: "moderate".into(),
                evidence_count: 1,
                first_seen_ns: 1,
                last_seen_ns: 1,
                metadata_json: "{}".into(),
            })
            .expect("edge");
    }
    let mut ids: Vec<_> = (0..503)
        .map(|index| format!("service:batch-{index}.service"))
        .collect();
    ids.push(ids[501].clone());
    ids.push("service:missing'; SELECT secret.service".into());
    let refs: Vec<_> = ids.iter().map(String::as_str).collect();
    let batch = store.list_active_edges_to_nodes(&refs).expect("batch");
    assert!(store
        .list_active_edges_to_nodes(&[])
        .expect("empty")
        .is_empty());
    for id in ids {
        let expected = store.list_active_edges_to(&id).expect("single");
        let actual = batch.get(&id).cloned().unwrap_or_default();
        assert_eq!(
            actual.iter().map(|row| &row.id).collect::<Vec<_>>(),
            expected.iter().map(|row| &row.id).collect::<Vec<_>>()
        );
    }
    assert_eq!(batch.len(), 502);
}
