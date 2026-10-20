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

#[test]
fn retention_keeps_the_state_at_the_cutoff_for_historical_reads() {
    let mut store = support::blank_store();
    support::seed_ab_nodes(&mut store);
    let edge = support::edge_row("a->parent_of->b", "a", "b", "parent_of", 100);
    store.upsert_edge(&edge).unwrap();
    let node = support::node_row("a", "process", "a", 100);
    for timestamp in [100, 150] {
        store
            .insert_node_history(&node, ChangeKind::Changed, timestamp, None)
            .unwrap();
        store
            .insert_edge_history(&edge, ChangeKind::Changed, timestamp, None)
            .unwrap();
    }
    let mut gone = support::edge_row("a->owns->b", "a", "b", "owns", 100);
    gone.state = "gone".into();
    store
        .insert_edge_history(&gone, ChangeKind::Gone, 120, None)
        .unwrap();
    let counts = store.prune_retention(0, 0, 200).unwrap();
    assert_eq!((counts.node_history, counts.edge_history), (1, 2));
    let at = twin_core::TimestampNs::new(250);
    assert_eq!(store.nodes_at(at).unwrap().len(), 1);
    let edges = store.edges_at(at).unwrap();
    assert_eq!(edges.len(), 1, "unchanged edge stays visible: {edges:?}");
    assert_eq!(edges[0].id, edge.id);
}

#[test]
fn retention_drops_old_collector_runs_nothing_cites_but_keeps_the_newest() {
    let mut store = support::blank_store();
    let mut runs = Vec::new();
    for (collector, ended) in [("proc", 10), ("proc", 20), ("proc", 30), ("systemd", 15)] {
        runs.push(
            store
                .insert_collector_run(&support::collector_run_row(collector, ended, ended))
                .unwrap(),
        );
    }
    let mut cited = support::observation_row("cited", "proc", "process_seen", 300);
    cited.collector_run_id = Some(runs[0]);
    store.insert_observation(&cited).unwrap();
    let counts = store.prune_retention(100, 100, 100).unwrap();
    assert_eq!(
        counts.collector_runs, 1,
        "only the uncited, superseded proc run"
    );
    let mut kept: Vec<_> = store
        .list_collector_runs(None)
        .unwrap()
        .into_iter()
        .filter_map(|run| run.id)
        .collect();
    kept.sort_unstable();
    assert_eq!(kept, [runs[0], runs[2], runs[3]]);
}
