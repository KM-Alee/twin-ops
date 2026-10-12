mod support;

use twin_app::{InitRequest, ScanRequest};

#[test]
fn why_is_deterministic_and_cites_observations_without_writing() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).unwrap();
    support::write_proc_fixture_with_cgroup(&proc, 42,
        "42 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n", b"api\0", None, Some("0::/system.slice/api.service\n"));
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).unwrap();
    let request = twin_app::why_request("service:api.service", None).unwrap();
    let store = twin_store::Store::open(&home.layout.db_file()).unwrap();
    let before = store.list_node_history_since(0).unwrap();
    let first = twin_app::why_in(&home.layout, request.clone()).unwrap();
    let second = twin_app::why_in(&home.layout, request).unwrap();
    assert_eq!(
        serde_json::to_string(&first).unwrap(),
        serde_json::to_string(&second).unwrap()
    );
    assert!(first
        .relationships
        .iter()
        .any(|relationship| relationship.class == "inferred"));
    assert!(first
        .relationships
        .iter()
        .any(|relationship| !relationship.observations.is_empty()));
    assert!(first
        .relationships
        .iter()
        .flat_map(|relationship| &relationship.observations)
        .any(|observation| observation.raw_ref.is_some()));
    assert_eq!(store.list_node_history_since(0).unwrap(), before);
    assert!(twin_app::why_in(
        &home.layout,
        twin_app::why_request("service:missing.service", None).unwrap()
    )
    .is_err());
}

#[test]
fn competing_service_ownership_is_reported_and_lowers_confidence() {
    use twin_core::{GraphEdge, GraphNode, NodeId, TimestampNs};
    let _guard = support::lock_scan_env();
    support::clear_scan_env();
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).unwrap();
    support::write_proc_fixture_with_cgroup(&proc, 42,
        "42 (api) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n", b"api\0", None, Some("0::/system.slice/api.service\n"));
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).unwrap();
    let request = twin_app::why_request("service:api.service", None).unwrap();
    let before = twin_app::why_in(&home.layout, request.clone()).unwrap();
    let mut store = twin_store::Store::open(&home.layout.db_file()).unwrap();
    let seen = TimestampNs::now();
    store
        .upsert_node_typed(&GraphNode::service("other.service", seen, None))
        .unwrap();
    store
        .upsert_edge_typed(&GraphEdge::inferred_service_owns_process(
            &NodeId::service("other.service"),
            &NodeId::process(42),
            seen,
            None,
        ))
        .unwrap();
    let after = twin_app::why_in(&home.layout, request).unwrap();
    let original = before
        .relationships
        .iter()
        .find(|edge| edge.to == "process:pid:42")
        .unwrap();
    let conflicting = after
        .relationships
        .iter()
        .find(|edge| edge.id == original.id)
        .unwrap();
    assert!(conflicting.evidence_score < original.evidence_score);
    assert!(conflicting
        .unknowns
        .iter()
        .any(|unknown| unknown.contains("competing active service ownership")));
}

#[test]
fn why_explains_config_to_runtime_ownership_chain_without_arbitrary_metadata() {
    use twin_store::{EdgeRow, NodeRow, ObservationRow, Store};
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let now = twin_core::TimestampNs::now().as_i64();
    for (id, kind) in [
        ("service:nginx.service", "service"),
        ("file:/etc/nginx/nginx.conf", "file"),
        ("port:tcp:127.0.0.1:8000", "port"),
        ("process:pid:42", "process"),
        ("service:api.service", "service"),
    ] {
        store
            .upsert_node(&NodeRow {
                id: id.into(),
                kind: kind.into(),
                label: id.into(),
                state: "active".into(),
                first_seen_ns: now,
                last_seen_ns: now,
                valid_from_ns: now,
                valid_to_ns: None,
                metadata_json: "{}".into(),
            })
            .expect("node");
    }
    for (index, (from, kind, to, class, obs_kind, source)) in [
        (
            "service:nginx.service",
            "configured_by",
            "file:/etc/nginx/nginx.conf",
            "observed",
            "service_configured_by_file",
            "config_file_discovery",
        ),
        (
            "file:/etc/nginx/nginx.conf",
            "references",
            "port:tcp:127.0.0.1:8000",
            "observed",
            "config_proxy_pass",
            "config_file_discovery",
        ),
        (
            "process:pid:42",
            "listens_on",
            "port:tcp:127.0.0.1:8000",
            "observed",
            "tcp_socket_seen",
            "proc_net_tcp",
        ),
        (
            "service:api.service",
            "owns",
            "process:pid:42",
            "inferred",
            "process_belongs_to_cgroup",
            "proc_cgroup",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let edge_id = format!("{from}|{kind}|{to}");
        store
            .upsert_edge(&EdgeRow {
                id: edge_id.clone(),
                from_node_id: from.into(),
                to_node_id: to.into(),
                kind: kind.into(),
                class: class.into(),
                state: "active".into(),
                evidence_score: 70,
                evidence_label: "strong".into(),
                evidence_count: 1,
                first_seen_ns: now,
                last_seen_ns: now,
                metadata_json: "{}".into(),
            })
            .expect("edge");
        let observation = ObservationRow {
            id: format!("00000000-0000-4000-8000-{index:012}"), source:source.into(),kind:obs_kind.into(),
            subject_node_id:Some(from.into()),object_node_id:Some(to.into()),timestamp_ns:now,
            confidence_hint:"high".into(),redaction_state:"none".into(),collector_run_id:None,
            metadata_json:serde_json::json!({"pid":42,"target":to,"line":3,"password":"NEVER_DISPLAY_SECRET","arbitrary":"NEVER_DISPLAY_ARBITRARY","__raw_ref":"/etc/nginx/nginx.conf"}).to_string(),
        };
        store.insert_observation(&observation).expect("observation");
        store
            .link_edge_observation(&edge_id, &observation.id, "direct")
            .expect("link");
    }
    let result = twin_app::why_in(
        &home.layout,
        twin_app::why_request("nginx", None).expect("request"),
    )
    .expect("why");
    let chain = result
        .proof_chains
        .iter()
        .find(|chain| {
            chain
                .nodes
                .last()
                .is_some_and(|id| id.as_str() == "service:api.service")
        })
        .expect("full config/runtime chain");
    assert_eq!(chain.relationship_ids.len(), 4);
    assert_eq!(chain.class, "inferred");
    assert_eq!(chain.nodes[1].as_str(), "file:/etc/nginx/nginx.conf");
    assert_eq!(chain.nodes[2].as_str(), "port:tcp:127.0.0.1:8000");
    let json = serde_json::to_string(&result).expect("json");
    assert!(!json.contains("NEVER_DISPLAY"));
    assert!(json.contains("pid=42"));
}

#[test]
fn why_bounds_large_neighborhoods_and_never_scores_unretained_evidence() {
    use twin_core::{GraphEdge, GraphNode, NodeId, TimestampNs};
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    let now = TimestampNs::now();
    store
        .upsert_node_typed(&GraphNode::service("api.service", now, None))
        .expect("node");
    for pid in 1..80 {
        store
            .upsert_node(&twin_store::NodeRow {
                id: NodeId::process(pid).to_string(),
                kind: "process".into(),
                label: format!("worker {pid}"),
                state: "active".into(),
                first_seen_ns: now.as_i64(),
                last_seen_ns: now.as_i64(),
                valid_from_ns: now.as_i64(),
                valid_to_ns: None,
                metadata_json: "{}".into(),
            })
            .expect("process");
        store
            .upsert_edge_typed(&GraphEdge::inferred_service_owns_process(
                &NodeId::service("api.service"),
                &NodeId::process(pid),
                now,
                None,
            ))
            .expect("edge");
    }
    let result = twin_app::why_in(
        &home.layout,
        twin_app::why_request("api", None).expect("request"),
    )
    .expect("why");
    assert!(result.relationships.len() <= 31);
    assert!(result.proof_chains.len() <= 16);
    assert!(result
        .unknowns
        .iter()
        .any(|unknown| unknown.contains("capped")));
    assert!(result
        .relationships
        .iter()
        .all(|relationship| relationship.evidence_score == 0));
}

#[test]
fn old_observation_is_stale_even_when_latest_scan_ended_at_the_same_time() {
    use twin_core::{GraphEdge, GraphNode, NodeId, TimestampNs};
    use twin_store::{CollectorRunRow, ObservationRow, Store};
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let old = TimestampNs::new(TimestampNs::now().as_i64() - 7_200_000_000_000);
    let service = NodeId::service("api.service");
    let process = NodeId::process(42);
    store
        .upsert_node_typed(&GraphNode::service("api.service", old, None))
        .expect("service");
    store
        .upsert_node_typed(&GraphNode::process(42, "api", old, None))
        .expect("process");
    let edge = GraphEdge::inferred_service_owns_process(&service, &process, old, None);
    store.upsert_edge_typed(&edge).expect("edge");
    let run = store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "proc_process".into(),
            started_at_ns: old.as_i64(),
            ended_at_ns: old.as_i64(),
            status: "success".into(),
            observation_count: 1,
            warning_count: 0,
            error_message: None,
            metadata_json: r#"{"readable_processes":1}"#.into(),
        })
        .expect("old scan");
    let observation = ObservationRow {
        id: "00000000-0000-4000-8000-000000000042".into(),
        source: "proc_cgroup".into(),
        kind: "process_belongs_to_cgroup".into(),
        subject_node_id: Some(process.to_string()),
        object_node_id: Some(service.to_string()),
        timestamp_ns: old.as_i64(),
        confidence_hint: "high".into(),
        redaction_state: "none".into(),
        metadata_json: "{}".into(),
        collector_run_id: Some(run),
    };
    store.insert_observation(&observation).expect("observation");
    store
        .link_edge_observation(&edge.id().to_string(), &observation.id, "direct")
        .expect("link");
    let result = twin_app::why_in(
        &home.layout,
        twin_app::why_request("api.service", None).expect("request"),
    )
    .expect("why");
    assert!(result
        .relationships
        .iter()
        .any(|relationship| relationship.unknowns.iter().any(
            |reason| reason == "supporting evidence is old or its observation time is unknown"
        )));
}
