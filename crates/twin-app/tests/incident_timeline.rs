mod support;

use twin_app::{InitRequest, WhatChangedRequest};
use twin_core::{ChangeCategory, ChangeKind, TimestampNs};
use twin_store::{EdgeRow, NodeRow, Store};

fn request() -> WhatChangedRequest {
    WhatChangedRequest {
        config_override: None,
        since: "1h".into(),
        verbose: false,
        target_query: Some("api".into()),
        categories: vec![],
        limit: 50,
        max_depth: 2,
    }
}

fn node(id: &str, kind: &str, time: i64) -> NodeRow {
    NodeRow {
        id: id.into(),
        kind: kind.into(),
        label: id.into(),
        state: "active".into(),
        first_seen_ns: time,
        last_seen_ns: time,
        valid_from_ns: time,
        valid_to_ns: None,
        metadata_json: "{}".into(),
    }
}

#[test]
fn target_timeline_uses_disappeared_links_and_filters_newest_entries() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    let now = TimestampNs::now().as_i64();
    for (id, kind, offset) in [
        ("service:api.service", "service", 50),
        ("process:pid:11", "process", 40),
        ("process:pid:12", "process", 20),
        ("process:pid:99", "process", 10),
    ] {
        let row = node(id, kind, now - offset);
        store.upsert_node(&row).expect("node");
        store
            .insert_node_history(&row, ChangeKind::New, now - offset, None)
            .expect("history");
    }
    for pid in [11, 12] {
        let edge = EdgeRow {
            id: format!("service:api.service|owns|process:pid:{pid}"),
            from_node_id: "service:api.service".into(),
            to_node_id: format!("process:pid:{pid}"),
            kind: "owns".into(),
            class: "observed".into(),
            state: "gone".into(),
            evidence_score: 60,
            evidence_label: "moderate".into(),
            evidence_count: 1,
            first_seen_ns: now - 60,
            last_seen_ns: now - 5,
            metadata_json: "{}".into(),
        };
        store
            .insert_edge_history(&edge, ChangeKind::Gone, now - 5, None)
            .expect("gone ownership");
    }
    let result = twin_app::what_changed_in(&home.layout, request()).expect("timeline");
    assert_eq!(result.target.as_deref(), Some("service:api.service"));
    assert_eq!(result.operational_changes.len(), 2);
    assert_eq!(
        result.operational_changes[0].entity.as_str(),
        "process:pid:11"
    );
    assert_eq!(
        result.operational_changes[1].entity.as_str(),
        "process:pid:12"
    );
    assert!(!result
        .new_nodes
        .iter()
        .any(|row| row.id == "process:pid:99"));
    let mut filtered = request();
    filtered.categories = vec![ChangeCategory::ProcessStarted];
    filtered.limit = 1;
    let result = twin_app::what_changed_in(&home.layout, filtered).expect("filtered");
    assert_eq!(result.operational_changes.len(), 1);
    assert_eq!(
        result.operational_changes[0].entity.as_str(),
        "process:pid:12"
    );
    assert_eq!(result.omitted_operational_changes, 1);
}

#[test]
fn invalid_history_queries_fail_and_missing_targets_do_not_return_global_changes() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    assert!(twin_app::what_changed_in(&home.layout, request()).is_err());
    let mut invalid = request();
    invalid.limit = 0;
    assert!(twin_app::what_changed_in(&home.layout, invalid)
        .unwrap_err()
        .to_string()
        .contains("limit"));
    let mut invalid = request();
    invalid.max_depth = 5;
    assert!(twin_app::what_changed_in(&home.layout, invalid).is_err());
}
