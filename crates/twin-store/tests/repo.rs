mod support;

use std::str::FromStr;

use twin_core::{CollectorName, ObservationId, TimestampNs};
use twin_observation::{
    ConfidenceHint, ObservationKind, ObservationMetadata, ObservationSource, Pipeline,
    RawEvidenceRef, RawIdentity, RawObservation,
};
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
fn batch_get_nodes_and_observations_by_ids() {
    let mut store = support::blank_store();
    seed_ab_nodes(&mut store);
    store
        .insert_observation(&observation_row("obs-a", "proc", "ProcessSeen", TS))
        .expect("obs");
    store
        .insert_observation(&observation_row("obs-b", "proc", "ProcessSeen", TS + 1))
        .expect("obs");

    assert_eq!(store.count_nodes_by_kind("process").expect("count"), 2);
    let nodes = store.get_nodes_by_ids(&["a", "missing"]).expect("nodes");
    assert_eq!(nodes.len(), 1);
    assert!(nodes.contains_key("a"));

    let obs = store
        .get_observations_by_ids(&["obs-a", "obs-b"])
        .expect("obs");
    assert_eq!(obs.len(), 2);
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
fn link_edge_observation_duplicate_is_noop() {
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
        .expect("link1");
    store
        .link_edge_observation("e1", "obs-1", "direct")
        .expect("link2");
    let links = store.list_observations_for_edge("e1").expect("list");
    assert_eq!(links.len(), 1);
}

#[test]
fn delete_edge_removes_edge_and_links() {
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
    store.delete_edge("e1").expect("delete");
    assert!(store.get_edge("e1").expect("get").is_none());
    assert!(store
        .list_observations_for_edge("e1")
        .expect("list")
        .is_empty());
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
fn observation_typed_roundtrip() {
    let mut store = support::blank_store();
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("note", "test");
    let raw = RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessSeen,
        collector: CollectorName::new("proc"),
        subject: Some(RawIdentity::Process { pid: 7 }),
        object: None,
        timestamp: TimestampNs::new(TS),
        raw_ref: Some(RawEvidenceRef::new("/proc/7/status")),
        confidence_hint: ConfidenceHint::Moderate,
        metadata,
    };
    let obs = Pipeline::default().process(raw).expect("pipeline");
    let id = obs.id();
    store.insert_observation_typed(&obs).expect("insert typed");
    let got = store
        .get_observation_typed(id)
        .expect("get typed")
        .expect("row");
    assert_eq!(got, obs);
}

#[test]
fn typed_insert_visible_to_row_api() {
    let mut store = support::blank_store();
    let raw = RawObservation {
        source: ObservationSource::ProcNetTcp,
        kind: ObservationKind::TcpSocketSeen,
        collector: CollectorName::new("proc"),
        subject: None,
        object: None,
        timestamp: TimestampNs::new(TS),
        raw_ref: Some(RawEvidenceRef::new("proc/net/tcp:1")),
        confidence_hint: ConfidenceHint::Low,
        metadata: ObservationMetadata::new(),
    };
    let obs = Pipeline::default().process(raw).expect("pipeline");
    let id = obs.id().to_string();
    store.insert_observation_typed(&obs).expect("insert");
    let row = store.get_observation(&id).expect("get").expect("row");
    assert_eq!(row.source, "proc_net_tcp");
    assert_eq!(row.kind, "tcp_socket_seen");
    assert!(row.metadata_json.contains("__raw_ref"));
}

fn insert_row_and_decode_err(mut row: twin_store::ObservationRow) -> StoreError {
    let mut store = support::blank_store();
    row.id = ObservationId::new().to_string();
    store.insert_observation(&row).expect("insert");
    let id = ObservationId::from_str(&row.id).expect("id");
    store.get_observation_typed(id).expect_err("decode")
}

#[test]
fn decode_bad_source_errors() {
    let row = observation_row("x", "not-a-real-source", "process_seen", TS);
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn decode_bad_kind_errors() {
    let row = observation_row("x", "proc", "not_a_kind", TS);
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn decode_bad_redaction_state_errors() {
    let mut row = observation_row("x", "proc", "process_seen", TS);
    row.redaction_state = "bogus".to_string();
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn decode_bad_metadata_json_errors() {
    let mut row = observation_row("x", "proc", "process_seen", TS);
    row.metadata_json = "[]".to_string();
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn decode_bad_subject_node_id_errors() {
    let mut row = observation_row("x", "proc", "process_seen", TS);
    row.subject_node_id = Some("not-a-node".to_string());
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn decode_bad_raw_ref_type_errors() {
    let mut row = observation_row("x", "proc", "process_seen", TS);
    row.metadata_json = r#"{"__raw_ref": 1}"#.to_string();
    assert!(matches!(
        insert_row_and_decode_err(row),
        StoreError::Decode { .. }
    ));
}

#[test]
fn raw_ref_metadata_collision_uses_typed_field() {
    let mut store = support::blank_store();
    let mut metadata = ObservationMetadata::new();
    metadata.insert_str("__raw_ref", "from-metadata");
    let raw = RawObservation {
        source: ObservationSource::Proc,
        kind: ObservationKind::ProcessSeen,
        collector: CollectorName::new("proc"),
        subject: None,
        object: None,
        timestamp: TimestampNs::new(TS),
        raw_ref: Some(RawEvidenceRef::new("from-field")),
        confidence_hint: ConfidenceHint::Moderate,
        metadata,
    };
    let obs = Pipeline::default().process(raw).expect("pipeline");
    store.insert_observation_typed(&obs).expect("insert");
    let row = store
        .get_observation(&obs.id().to_string())
        .expect("get")
        .expect("row");
    assert!(row.metadata_json.contains("from-field"));
    assert!(!row.metadata_json.contains("from-metadata"));
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
