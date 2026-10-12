mod support;

use twin_core::{
    EdgeClass, EdgeKind, EdgeState, GraphEdge, GraphEdgeParts, GraphMetadata, GraphNode,
    GraphNodeParts, NodeId, NodeState, TimestampNs,
};
use twin_store::{CollectorRunRow, Store};

fn node(id: &str, metadata: serde_json::Value, at: TimestampNs) -> GraphNode {
    let id: NodeId = id.parse().unwrap();
    GraphNode::from_parts(GraphNodeParts {
        kind: id.kind().unwrap(),
        label: id.to_string(),
        id,
        state: NodeState::Active,
        first_seen: at,
        last_seen: at,
        valid_from: at,
        valid_to: None,
        metadata: GraphMetadata::from_json(metadata.to_string()),
    })
}
fn collector(store: &mut Store, name: &str, at: TimestampNs) {
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: name.into(),
            started_at_ns: at.as_i64(),
            ended_at_ns: at.as_i64(),
            status: "success".into(),
            observation_count: 1,
            warning_count: 0,
            error_message: None,
            metadata_json: "{}".into(),
        })
        .unwrap();
}
fn owns(store: &mut Store, from: &str, to: &str, at: TimestampNs) {
    store
        .upsert_edge_typed(&GraphEdge::from_stored(GraphEdgeParts {
            from: from.parse().unwrap(),
            to: to.parse().unwrap(),
            kind: EdgeKind::Owns,
            class: EdgeClass::Observed,
            state: EdgeState::Active,
            evidence_count: 1,
            first_seen: at,
            last_seen: at,
            metadata: GraphMetadata::from_json("{}"),
        }))
        .unwrap();
}

#[test]
fn stored_contract_checks_container_health_and_actual_routed_deployment() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let at = TimestampNs::now();
    for source in [twin_collectors::COLLECTOR_NAME, "docker", "k8s"] {
        collector(&mut store, source, at);
    }
    store
        .upsert_node_typed(&node(
            "container:api",
            serde_json::json!({"is_running":true,"health_status":"healthy"}),
            at,
        ))
        .unwrap();
    store.upsert_node_typed(&node("k8s:deployment:default/api",serde_json::json!({"desired_replicas":1,"ready_replicas":1,"readiness_known":true,"generation":2,"observed_generation":2}),at)).unwrap();
    store
        .upsert_node_typed(&node(
            "k8s:replicaset:default/api-1",
            serde_json::json!({}),
            at,
        ))
        .unwrap();
    store
        .upsert_node_typed(&node(
            "k8s:pod:default/api-1",
            serde_json::json!({"ready":true}),
            at,
        ))
        .unwrap();
    store
        .upsert_node_typed(&node(
            "k8s:service:default/api",
            serde_json::json!({"ready_pods":["api-1"],"ready_pods_known":true}),
            at,
        ))
        .unwrap();
    owns(
        &mut store,
        "k8s:deployment:default/api",
        "k8s:replicaset:default/api-1",
        at,
    );
    owns(
        &mut store,
        "k8s:replicaset:default/api-1",
        "k8s:pod:default/api-1",
        at,
    );
    let path = home.layout.data_dir.join("health.yaml");
    std::fs::write(&path,"version: 1\nname: health\ndefaults:\n  fail_on_unknown: true\nchecks:\n  - name: container healthy\n    assert:\n      container:\n        id: container:api\n        healthy: true\n  - name: deployment routed\n    assert:\n      k8s_ready:\n        deployment: k8s:deployment:default/api\n        routed_service: k8s:service:default/api\n").unwrap();
    assert_eq!(twin_app::run_file(&home.layout, &path).unwrap().passed, 2);
    store
        .upsert_node_typed(&node(
            "k8s:service:default/api",
            serde_json::json!({"ready_pods":["api-1"],"ready_pods_known":false}),
            at,
        ))
        .unwrap();
    store
        .upsert_node_typed(&node(
            "container:api",
            serde_json::json!({"is_running":true,"health_status":"unhealthy"}),
            at,
        ))
        .unwrap();
    let report = twin_app::run_file(&home.layout, &path).unwrap();
    assert_eq!(report.failed, 2);
    assert!(report.checks[1]
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:"));
    store
        .upsert_node_typed(&node(
            "container:api",
            serde_json::json!({"is_running":true,"health_status":"healthy"}),
            at,
        ))
        .unwrap();
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "docker".into(),
            started_at_ns: at.as_i64() + 1,
            ended_at_ns: at.as_i64() + 1,
            status: "unavailable".into(),
            observation_count: 0,
            warning_count: 1,
            error_message: None,
            metadata_json: "{}".into(),
        })
        .unwrap();
    assert_eq!(twin_app::run_file(&home.layout, &path).unwrap().failed, 2);
}

#[test]
fn unscanned_kubernetes_absence_is_unknown_despite_complete_host_scan() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    collector(
        &mut store,
        twin_collectors::COLLECTOR_NAME,
        TimestampNs::now(),
    );
    let path = home.layout.data_dir.join("absence.yaml");
    std::fs::write(&path,"version: 1\nname: absence\ndefaults:\n  fail_on_unknown: true\nchecks:\n  - name: no pod\n    assert:\n      node: k8s:pod:default/missing\n      exists: false\n").unwrap();
    let report = twin_app::run_file(&home.layout, &path).unwrap();
    assert_eq!(report.failed, 1);
    assert!(report.checks[0]
        .detail
        .as_deref()
        .unwrap()
        .starts_with("Unknown:"));
}
