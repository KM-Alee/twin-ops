mod support;

use twin_store::StoreError;

use support::{
    assert_foreign_key_err, collector_run_row, edge_row, edge_row_with_evidence, node_row,
    observation_row, seed_ab_nodes, TS,
};

#[test]
fn repo_roundtrips_and_list_filters() {
    let mut store = support::blank_store();

    let obs = {
        let mut row = observation_row("obs-1", "proc", "ProcessSeen", TS);
        row.subject_node_id = Some("process:pid:1".to_string());
        row
    };
    store.insert_observation(&obs).expect("insert obs");
    store
        .insert_observation(&observation_row("obs-2", "systemd", "UnitSeen", TS))
        .expect("insert other source");

    seed_ab_nodes(&mut store);
    let edge = edge_row("e1", "a", "b", "depends_on", TS);
    store.upsert_edge(&edge).expect("edge");

    assert_eq!(store.get_observation("obs-1").expect("get"), Some(obs));
    assert_eq!(
        store
            .list_observations_by_source("proc")
            .expect("list")
            .len(),
        1
    );
    assert_eq!(
        store
            .list_observations_by_subject("process:pid:1")
            .expect("list")
            .len(),
        1
    );
    assert_eq!(
        store.get_node("a").expect("get").map(|n| n.id),
        Some("a".to_string())
    );
    assert_eq!(store.list_nodes_by_kind("process").expect("list").len(), 2);
    assert_eq!(store.get_edge("e1").expect("get"), Some(edge));
    assert_eq!(store.list_edges_from("a").expect("list").len(), 1);
    assert_eq!(store.list_edges_to("b").expect("list").len(), 1);
    assert_eq!(
        store.list_edges_by_kind("depends_on").expect("list").len(),
        1
    );
}

#[test]
fn upsert_node_preserves_first_seen_updates_last_seen() {
    let mut store = support::blank_store();
    store
        .upsert_node(&node_row("n1", "process", "init", TS))
        .expect("first");
    let mut second = node_row("n1", "process", "init", TS + 100);
    second.label = "updated".to_string();
    store.upsert_node(&second).expect("second");
    let got = store.get_node("n1").expect("get").expect("row");
    assert_eq!(got.first_seen_ns, TS);
    assert_eq!(got.last_seen_ns, TS + 100);
    assert_eq!(got.label, "updated");
    assert_eq!(store.count_nodes().expect("count"), 1);
}

#[test]
fn upsert_edge_uses_row_values_on_conflict() {
    let mut store = support::blank_store();
    seed_ab_nodes(&mut store);
    store
        .upsert_edge(&edge_row("e1", "a", "b", "depends_on", TS))
        .expect("first");
    store
        .upsert_edge(&edge_row_with_evidence(
            "e1",
            "a",
            "b",
            "depends_on",
            TS + 1,
            3,
        ))
        .expect("second");
    let got = store.get_edge("e1").expect("get").expect("row");
    assert_eq!(got.evidence_count, 3);
    assert_eq!(got.last_seen_ns, TS + 1);
    assert_eq!(got.first_seen_ns, TS);
    assert_eq!(store.count_edges().expect("count"), 1);
}

#[test]
fn insert_observations_bulk_commits_all_rows() {
    let mut store = support::blank_store();
    let rows = vec![
        observation_row("o1", "proc", "A", TS),
        observation_row("o2", "proc", "B", TS + 1),
        observation_row("o3", "proc", "C", TS + 2),
    ];
    store.insert_observations(&rows).expect("bulk");
    assert_eq!(store.count_observations().expect("count"), 3);
    for row in rows {
        assert_eq!(store.get_observation(&row.id).expect("get"), Some(row));
    }
}

#[test]
fn insert_observations_bulk_rolls_back_on_failure() {
    let mut store = support::blank_store();
    let mut bad = observation_row("o3", "proc", "C", TS + 2);
    bad.collector_run_id = Some(999);
    let rows = vec![
        observation_row("o1", "proc", "A", TS),
        observation_row("o2", "proc", "B", TS + 1),
        bad,
    ];
    let err = store.insert_observations(&rows).expect_err("fk");
    assert_foreign_key_err(err);
    assert_eq!(store.count_observations().expect("count"), 0);
}

#[test]
fn link_edge_observation_roundtrip() {
    let mut store = support::blank_store();
    seed_ab_nodes(&mut store);
    store
        .upsert_edge(&edge_row("e1", "a", "b", "depends_on", TS))
        .expect("edge");
    store
        .insert_observation(&observation_row("obs-1", "proc", "A", TS))
        .expect("obs");
    store
        .link_edge_observation("e1", "obs-1", "direct")
        .expect("link");
    let links = store.list_observations_for_edge("e1").expect("list");
    assert_eq!(links, vec![("obs-1".to_string(), "direct".to_string())]);
}

#[test]
fn collector_run_insert_list_and_latest() {
    let mut store = support::blank_store();
    let id = store
        .insert_collector_run(&collector_run_row("proc", TS, TS + 1))
        .expect("insert");
    assert!(id > 0);
    store
        .insert_collector_run(&collector_run_row("proc", TS + 10, TS + 11))
        .expect("second");
    store
        .insert_collector_run(&collector_run_row("systemd", TS, TS + 1))
        .expect("other collector");
    assert_eq!(
        store.list_collector_runs(Some("proc")).expect("list").len(),
        2
    );
    assert_eq!(store.list_collector_runs(None).expect("list").len(), 3);
    let latest = store
        .latest_collector_run("proc")
        .expect("latest")
        .expect("row");
    assert_eq!(latest.started_at_ns, TS + 10);
}

#[test]
fn get_nonexistent_returns_none() {
    let store = support::blank_store();
    assert!(store.get_node("nope").expect("get").is_none());
    assert!(store.get_edge("nope").expect("get").is_none());
    assert!(store.get_observation("nope").expect("get").is_none());
}

#[test]
fn foreign_key_violations_rejected() {
    let mut store = support::blank_store();
    seed_ab_nodes(&mut store);
    store
        .upsert_edge(&edge_row("e1", "a", "b", "depends_on", TS))
        .expect("edge");

    let edge_fk = store
        .upsert_edge(&edge_row("e-missing", "missing", "b", "depends_on", TS))
        .expect_err("edge fk");
    assert_foreign_key_err(edge_fk);

    let link_fk = store
        .link_edge_observation("e1", "missing-obs", "direct")
        .expect_err("link fk");
    assert_foreign_key_err(link_fk);

    let mut obs = observation_row("o1", "proc", "A", TS);
    obs.collector_run_id = Some(999);
    let obs_fk = store.insert_observation(&obs).expect_err("obs fk");
    assert_foreign_key_err(obs_fk);
}
