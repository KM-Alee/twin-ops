mod support;

use twin_core::{EdgeId, TimestampNs};
use twin_store::Store;

#[test]
fn snapshot_keeps_proof_references_after_incarnation_reset_and_retention() {
    let mut store = support::blank_store();
    store
        .upsert_node(&support::node_row("process:pid:42", "process", "api", 10))
        .expect("node");
    store
        .upsert_edge(&support::edge_row(
            "process:pid:42|parent_of|process:pid:42",
            "process:pid:42",
            "process:pid:42",
            "parent_of",
            10,
        ))
        .expect("edge");
    let old = support::observation_row("old", "proc", "process_seen", 10);
    store.insert_observation(&old).expect("old");
    let edge = "process:pid:42|parent_of|process:pid:42";
    store
        .link_edge_observation(edge, "old", "direct")
        .expect("link");
    store.create_snapshot("baseline", 20).expect("snapshot");
    store
        .reset_edge_incarnation(&edge.parse::<EdgeId>().expect("id"), TimestampNs::new(30))
        .expect("reset");
    let new = support::observation_row("new", "proc", "process_seen", 30);
    store.insert_observation(&new).expect("new");
    store
        .link_edge_observation(edge, "new", "direct")
        .expect("new link");
    assert_eq!(
        store.list_observations_for_edge(edge).expect("current"),
        vec![("new".into(), "direct".into())]
    );
    assert_eq!(
        store
            .load_snapshot_observations_for_edge("baseline", edge)
            .expect("baseline"),
        vec![("old".into(), "direct".into())]
    );
    store.prune_retention(25, 25, 0).expect("retention");
    assert_eq!(
        store
            .load_snapshot_observations_for_edge("baseline", edge)
            .expect("retained references"),
        vec![("old".into(), "direct".into())]
    );
}

#[test]
fn v5_snapshot_backfill_excludes_post_snapshot_observations() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("v5.db");
    let mut store = Store::open(&path).expect("store");
    store.apply_migrations_through(5).expect("v5");
    store
        .upsert_node(&support::node_row("process:pid:42", "process", "api", 10))
        .expect("node");
    store
        .upsert_edge(&support::edge_row(
            "edge",
            "process:pid:42",
            "process:pid:42",
            "parent_of",
            10,
        ))
        .expect("edge");
    for (id, timestamp) in [("old", 10), ("future", 30)] {
        store
            .insert_observation(&support::observation_row(
                id,
                "proc",
                "process_seen",
                timestamp,
            ))
            .expect("observation");
        store
            .link_edge_observation("edge", id, "direct")
            .expect("link");
    }
    let conn = rusqlite::Connection::open(&path).expect("fixture");
    conn.execute_batch(
        "INSERT INTO snapshots VALUES ('baseline',20,1,1);
INSERT INTO snapshot_nodes SELECT 'baseline', * FROM nodes;
INSERT INTO snapshot_edges SELECT 'baseline', * FROM edges;",
    )
    .expect("v5 snapshot");
    store.initialize().expect("upgrade");
    assert_eq!(
        store
            .load_snapshot_observations_for_edge("baseline", "edge")
            .expect("backfill"),
        vec![("old".into(), "direct".into())]
    );
    conn.execute("DELETE FROM snapshots WHERE name='baseline'", [])
        .expect("remove baseline");
    assert!(store
        .load_snapshot_observations_for_edge("baseline", "edge")
        .expect("cascade")
        .is_empty());
}
