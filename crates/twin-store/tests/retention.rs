mod support;

use twin_core::ChangeKind;

#[test]
fn retention_uses_independent_cutoffs_and_cleans_links_without_touching_graph_or_snapshots() {
    let mut store = support::blank_store();
    support::seed_ab_nodes(&mut store);
    let edge = support::edge_row("a->parent_of->b", "a", "b", "parent_of", 100);
    store.upsert_edge(&edge).unwrap();
    for (id, source, timestamp) in [
        ("old-proc", "proc", 99),
        ("boundary-proc", "proc", 100),
        ("old-ebpf", "ebpf", 199),
        ("boundary-ebpf", "ebpf", 200),
        ("recent", "proc", 300),
    ] {
        store
            .insert_observation(&support::observation_row(
                id,
                source,
                "process_seen",
                timestamp,
            ))
            .unwrap();
        store.link_edge_observation(&edge.id, id, "direct").unwrap();
    }
    let node = support::node_row("a", "process", "a", 100);
    for timestamp in [299, 300] {
        store
            .insert_node_history(&node, ChangeKind::Changed, timestamp, None)
            .unwrap();
        store
            .insert_edge_history(&edge, ChangeKind::Changed, timestamp, None)
            .unwrap();
    }
    store.create_snapshot("keep", 100).unwrap();
    let counts = store.prune_retention(100, 200, 300).unwrap();
    assert_eq!(counts.observations, 2);
    assert_eq!(counts.node_history, 1);
    assert_eq!(counts.edge_history, 1);
    let links = store.list_observations_for_edge(&edge.id).unwrap();
    assert_eq!(links.len(), 3);
    assert!(links.iter().all(|(id, _)| !id.starts_with("old-")));
    assert_eq!(
        store.list_node_history_since(0).unwrap()[0].recorded_at_ns,
        300
    );
    assert_eq!(
        store.list_edge_history_since(0).unwrap()[0].recorded_at_ns,
        300
    );
    assert_eq!(store.list_current_nodes().unwrap().len(), 2);
    assert_eq!(store.load_snapshot_edges("keep").unwrap().len(), 1);
    assert_eq!(
        store.prune_retention(100, 200, 300).unwrap().observations,
        0
    );
}
