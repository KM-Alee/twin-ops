use std::path::Path;

use twin_store::{
    is_foreign_key_violation, store_error_source, CollectorRunRow, EdgeRow, NodeRow,
    ObservationRow, Store, StoreError, LATEST_VERSION,
};

pub const TS: i64 = 1_700_000_000_000_000_000;

pub fn blank_store() -> Store {
    let store = Store::open_in_memory().expect("open in-memory DB");
    store.initialize().expect("run migrations");
    assert_eq!(store.schema_version().expect("version"), LATEST_VERSION);
    store
}

pub fn node_row(id: &str, kind: &str, label: &str, ts_ns: i64) -> NodeRow {
    NodeRow {
        id: id.to_string(),
        kind: kind.to_string(),
        label: label.to_string(),
        state: "active".to_string(),
        first_seen_ns: ts_ns,
        last_seen_ns: ts_ns,
        valid_from_ns: ts_ns,
        valid_to_ns: None,
        metadata_json: "{}".to_string(),
    }
}

pub fn edge_row(id: &str, from: &str, to: &str, kind: &str, ts_ns: i64) -> EdgeRow {
    edge_row_with_evidence(id, from, to, kind, ts_ns, 0)
}

pub fn edge_row_with_evidence(
    id: &str,
    from: &str,
    to: &str,
    kind: &str,
    ts_ns: i64,
    evidence_count: i64,
) -> EdgeRow {
    EdgeRow {
        id: id.to_string(),
        from_node_id: from.to_string(),
        to_node_id: to.to_string(),
        kind: kind.to_string(),
        class: "observed".to_string(),
        state: "active".to_string(),
        evidence_score: 0,
        evidence_label: "weak".to_string(),
        evidence_count,
        first_seen_ns: ts_ns,
        last_seen_ns: ts_ns,
        metadata_json: "{}".to_string(),
    }
}

pub fn observation_row(id: &str, source: &str, kind: &str, ts_ns: i64) -> ObservationRow {
    ObservationRow {
        id: id.to_string(),
        source: source.to_string(),
        kind: kind.to_string(),
        subject_node_id: None,
        object_node_id: None,
        timestamp_ns: ts_ns,
        confidence_hint: "moderate".to_string(),
        redaction_state: "none".to_string(),
        metadata_json: "{}".to_string(),
        collector_run_id: None,
    }
}

pub fn collector_run_row(collector: &str, started: i64, ended: i64) -> CollectorRunRow {
    CollectorRunRow {
        id: None,
        collector: collector.to_string(),
        started_at_ns: started,
        ended_at_ns: ended,
        status: "success".to_string(),
        observation_count: 0,
        warning_count: 0,
        error_message: None,
        metadata_json: "{}".to_string(),
    }
}

pub fn table_exists(path: &Path, name: &str) -> bool {
    let conn = rusqlite::Connection::open(path).expect("open for table check");
    conn.query_row(
        "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
        [name],
        |_| Ok(()),
    )
    .is_ok()
}

pub fn open_v1_only(path: &Path) -> Store {
    let store = Store::open(path).expect("open");
    store.apply_migrations_through(1).expect("v1");
    assert_eq!(store.schema_version().expect("version"), 1);
    store
}

pub fn open_v2_only(path: &Path) -> Store {
    let store = Store::open(path).expect("open");
    store.apply_migrations_through(2).expect("v2");
    assert_eq!(store.schema_version().expect("version"), 2);
    store
}

pub fn open_v3_only(path: &Path) -> Store {
    let store = Store::open(path).expect("open");
    store.apply_migrations_through(3).expect("v3");
    assert_eq!(store.schema_version().expect("version"), 3);
    store
}

pub fn column_exists(path: &Path, table: &str, column: &str) -> bool {
    let conn = rusqlite::Connection::open(path).expect("open");
    conn.query_row(
        "SELECT 1 FROM pragma_table_info(?1) WHERE name = ?2",
        [table, column],
        |_| Ok(()),
    )
    .is_ok()
}

pub fn assert_foreign_key_err(err: StoreError) {
    let source = store_error_source(&err).expect("rusqlite source");
    assert!(
        is_foreign_key_violation(source),
        "expected foreign key violation, got {source}"
    );
}

pub fn seed_ab_nodes(store: &mut Store) {
    store
        .upsert_node(&node_row("a", "process", "a", TS))
        .expect("node a");
    store
        .upsert_node(&node_row("b", "process", "b", TS))
        .expect("node b");
}
