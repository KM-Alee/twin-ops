mod support;

use twin_core::{EdgeId, NodeId, TimestampNs};
use twin_store::{ObservationRow, Store};

const PROCESS: &str = "process:pid:42";
const EDGE: &str = "process:pid:42|parent_of|process:pid:42";

fn store_with_process() -> Store {
    let mut store = support::blank_store();
    store
        .upsert_node(&support::node_row(PROCESS, "process", "api", 10))
        .expect("node");
    store
        .upsert_edge(&support::edge_row(EDGE, PROCESS, PROCESS, "parent_of", 10))
        .expect("edge");
    store
}

fn run(store: &mut Store, started: i64) -> i64 {
    store
        .insert_collector_run(&support::collector_run_row("proc", started, started))
        .expect("run")
}

fn sighting(id: &str, at: i64, run: Option<i64>, state: &str, line: u32) -> ObservationRow {
    let mut row = support::observation_row(id, "proc", "process_seen", at);
    row.subject_node_id = Some(PROCESS.into());
    row.collector_run_id = run;
    row.metadata_json = format!(
        r#"{{"__raw_ref":"/proc/net/unix:{line}","pid":42,"state":"{state}","raw_line":{line},"sample_at_ns":"{at}","sample_index":0}}"#
    );
    row
}

fn observe(store: &mut Store, row: &ObservationRow) {
    store.insert_observation(row).expect("insert");
    store
        .link_edge_observation(EDGE, &row.id, "direct")
        .expect("link");
}

fn merge(store: &mut Store) -> twin_store::ObservationMergeProgress {
    store.merge_all_repeated_observations(2).expect("merge")
}

fn rows(store: &Store) -> Vec<ObservationRow> {
    store
        .list_observations_by_subject(PROCESS)
        .expect("observations")
}

#[test]
fn consecutive_sightings_of_one_fact_update_a_single_row() {
    let mut store = store_with_process();
    for (index, at) in [20, 30, 40].into_iter().enumerate() {
        let run = run(&mut store, at);
        observe(
            &mut store,
            &sighting(
                &format!("obs-{index}"),
                at,
                Some(run),
                "S",
                index as u32 + 2,
            ),
        );
        merge(&mut store);
    }
    let rows = rows(&store);
    assert_eq!(rows.len(), 1, "{rows:?}");
    let row = &rows[0];
    assert_eq!(row.id, "obs-0");
    assert_eq!(row.timestamp_ns, 20);
    assert_eq!(row.last_seen_ns, 40);
    assert_eq!(row.seen_count, 3);
    assert!(
        row.metadata_json.contains("/proc/net/unix:4"),
        "newest provenance kept"
    );
    assert_eq!(
        store.list_observations_for_edge(EDGE).expect("links"),
        vec![("obs-0".into(), "direct".into())]
    );
    assert_eq!(store.count_observations().expect("count"), 1);
}

#[test]
fn a_changed_fact_keeps_its_own_row() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("sleeping", 20, Some(first), "S", 2));
    let second = run(&mut store, 30);
    observe(&mut store, &sighting("running", 30, Some(second), "R", 2));
    merge(&mut store);
    assert_eq!(rows(&store).len(), 2);
}

#[test]
fn a_fact_missing_from_a_run_reappears_as_a_new_row() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("before", 20, Some(first), "S", 2));
    merge(&mut store);
    run(&mut store, 30);
    let third = run(&mut store, 40);
    observe(&mut store, &sighting("after", 40, Some(third), "S", 2));
    let progress = merge(&mut store);
    assert_eq!(progress.merged, 0);
    let ids: Vec<_> = rows(&store).into_iter().map(|row| row.id).collect();
    assert_eq!(ids, ["before", "after"]);
}

#[test]
fn another_collector_run_in_between_does_not_break_continuity() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("first", 20, Some(first), "S", 2));
    store
        .insert_collector_run(&support::collector_run_row("systemd", 25, 25))
        .expect("other collector");
    let second = run(&mut store, 30);
    observe(&mut store, &sighting("second", 30, Some(second), "S", 3));
    assert_eq!(merge(&mut store).merged, 1);
}

#[test]
fn a_reborn_process_never_inherits_the_previous_incarnation() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(
        &mut store,
        &sighting("old-process", 20, Some(first), "S", 2),
    );
    merge(&mut store);
    store
        .reset_node_incarnation(
            &PROCESS.parse::<NodeId>().expect("id"),
            TimestampNs::new(25),
        )
        .expect("node reset");
    store
        .reset_edge_incarnation(&EDGE.parse::<EdgeId>().expect("id"), TimestampNs::new(25))
        .expect("edge reset");
    let second = run(&mut store, 30);
    observe(
        &mut store,
        &sighting("new-process", 30, Some(second), "S", 2),
    );
    assert_eq!(merge(&mut store).merged, 0);
    assert_eq!(
        store.list_observations_for_edge(EDGE).expect("links"),
        vec![("new-process".into(), "direct".into())]
    );
}

#[test]
fn a_sighting_supporting_different_relationships_is_kept_apart() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    let row = sighting("unlinked", 20, Some(first), "S", 2);
    store.insert_observation(&row).expect("insert");
    let second = run(&mut store, 30);
    observe(&mut store, &sighting("linked", 30, Some(second), "S", 2));
    assert_eq!(merge(&mut store).merged, 0);
    assert_eq!(rows(&store).len(), 2);
}

#[test]
fn events_without_runs_merge_only_with_other_events() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("scanned", 20, Some(first), "S", 2));
    observe(&mut store, &sighting("event-1", 30, None, "S", 2));
    observe(&mut store, &sighting("event-2", 40, None, "S", 2));
    let progress = merge(&mut store);
    assert_eq!(progress.merged, 1);
    let rows = rows(&store);
    let event = rows.iter().find(|row| row.id == "event-1").expect("event");
    assert_eq!(event.seen_count, 2);
    assert_eq!(event.last_seen_ns, 40);
}

#[test]
fn snapshot_proof_keeps_the_last_sighting_known_at_capture() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("fact", 20, Some(first), "S", 2));
    merge(&mut store);
    store.create_snapshot("baseline", 25).expect("snapshot");
    let second = run(&mut store, 30);
    observe(&mut store, &sighting("again", 30, Some(second), "S", 2));
    merge(&mut store);
    let proof = store
        .load_snapshot_observations_for_edge("baseline", EDGE)
        .expect("proof");
    assert_eq!(proof.len(), 1);
    assert_eq!(proof[0].observation_id, "fact");
    assert_eq!(proof[0].last_seen_ns, Some(20));
}

#[test]
fn retention_keeps_a_fact_that_is_still_being_seen() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    observe(&mut store, &sighting("fact", 20, Some(first), "S", 2));
    let second = run(&mut store, 100);
    observe(&mut store, &sighting("again", 100, Some(second), "S", 2));
    merge(&mut store);
    let pruned = store.prune_retention(50, 50, 0).expect("prune");
    assert_eq!(pruned.observations, 0);
    assert_eq!(rows(&store).len(), 1);
}

#[test]
fn migration_seven_collapses_legacy_duplicates_in_batches() {
    let directory = tempfile::tempdir().expect("directory");
    let path = directory.path().join("v6.db");
    let mut store = Store::open(&path).expect("store");
    store.apply_migrations_through(6).expect("v6");
    store
        .upsert_node(&support::node_row(PROCESS, "process", "api", 10))
        .expect("node");
    store
        .upsert_edge(&support::edge_row(EDGE, PROCESS, PROCESS, "parent_of", 10))
        .expect("edge");
    let conn = rusqlite::Connection::open(&path).expect("fixture");
    for (index, at) in [20, 30, 40, 50, 60].into_iter().enumerate() {
        let run = run(&mut store, at);
        let row = sighting(
            &format!("legacy-{index}"),
            at,
            Some(run),
            "S",
            index as u32 + 2,
        );
        conn.execute(
            "INSERT INTO observations VALUES (?1,'proc','process_seen',?2,NULL,?3,'moderate','none',?4,?5)",
            rusqlite::params![row.id, PROCESS, at, row.metadata_json, run],
        )
        .expect("legacy row");
        store
            .link_edge_observation(EDGE, &row.id, "direct")
            .expect("link");
    }
    conn.execute_batch(
        "INSERT INTO snapshots VALUES ('baseline',35,1,1);
INSERT INTO snapshot_nodes SELECT 'baseline', * FROM nodes;
INSERT INTO snapshot_edges SELECT 'baseline', * FROM edges;
INSERT INTO snapshot_edge_observations SELECT 'baseline', eo.edge_id, eo.observation_id, eo.role
FROM edge_observations eo JOIN observations o ON o.id = eo.observation_id WHERE o.timestamp_ns <= 35;",
    )
    .expect("legacy snapshot");
    store.initialize().expect("migrate");
    assert_eq!(store.schema_version().expect("version"), 7);
    let progress = merge(&mut store);
    assert_eq!(progress.merged, 4);
    assert_eq!(progress.pending, 0);
    let rows = rows(&store);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        (
            rows[0].timestamp_ns,
            rows[0].last_seen_ns,
            rows[0].seen_count
        ),
        (20, 60, 5)
    );
    let proof = store
        .load_snapshot_observations_for_edge("baseline", EDGE)
        .expect("proof");
    assert_eq!(proof.len(), 1, "{proof:?}");
    assert_eq!(proof[0].observation_id, "legacy-0");
    assert_eq!(proof[0].last_seen_ns, Some(30));
}

#[test]
fn a_node_reset_seals_unlinked_sightings_of_the_old_incarnation() {
    let mut store = store_with_process();
    let first = run(&mut store, 20);
    store
        .insert_observation(&sighting("old", 20, Some(first), "S", 2))
        .expect("old");
    merge(&mut store);
    let second = run(&mut store, 30);
    store
        .insert_observation(&sighting("new", 30, Some(second), "S", 2))
        .expect("new");
    store
        .reset_node_incarnation(
            &PROCESS.parse::<NodeId>().expect("id"),
            TimestampNs::new(30),
        )
        .expect("reset");
    assert_eq!(merge(&mut store).merged, 0);
    let third = run(&mut store, 40);
    store
        .insert_observation(&sighting("newer", 40, Some(third), "S", 2))
        .expect("newer");
    assert_eq!(merge(&mut store).merged, 1, "the new incarnation continues");
    let ids: Vec<_> = rows(&store).into_iter().map(|row| row.id).collect();
    assert_eq!(ids, ["old", "new"]);
}
