mod support;

use twin_app::{GraphRequest, GraphResult, InitRequest};
use twin_core::NodeId;
use twin_store::{EdgeRow, NodeRow, ObservationRow, Store};

#[test]
fn large_service_keeps_every_peer_label_proof_reference_and_valid_evidence() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let node = |id: String, kind: &str, label: String| NodeRow {
        id,
        kind: kind.into(),
        label,
        state: "active".into(),
        first_seen_ns: 1,
        last_seen_ns: 1,
        valid_from_ns: 1,
        valid_to_ns: None,
        metadata_json: "{}".into(),
    };
    store.with_transaction(|store| {
        store.upsert_node(&node("service:batch.service".into(), "service", "batch.service".into()))?;
        for pid in 1..=501 {
            let id = NodeId::process(pid).to_string();
            store.upsert_node(&node(id.clone(), "process", format!("worker label {pid}")))?;
            let edge = format!("service:batch.service|owns|{id}");
            store.upsert_edge(&EdgeRow { id: edge.clone(), from_node_id: "service:batch.service".into(), to_node_id: id.clone(), kind: "owns".into(), class: "inferred".into(), state: "active".into(), evidence_score: 45, evidence_label: "moderate".into(), evidence_count: 1, first_seen_ns: 1, last_seen_ns: 1, metadata_json: "{}".into() })?;
            let obs = format!("00000000-0000-0000-0000-{pid:012}");
            store.insert_observation(&ObservationRow { id: obs.clone(), source: "proc_cgroup".into(), kind: if pid == 501 { "malformed_kind" } else { "process_belongs_to_cgroup" }.into(), subject_node_id: Some(id), object_node_id: None, timestamp_ns: 1, confidence_hint: "moderate".into(), redaction_state: "none".into(), metadata_json: format!("{{\"cgroup_path\":\"/system.slice/batch.service/{pid}\",\"__raw_ref\":\"/proc/{pid}/cgroup\"}}"), collector_run_id: None , last_seen_ns: 0, seen_count: 1})?;
            store.link_edge_observation(&edge, &obs, "direct")?;
        }
        Ok(())
    }).expect("fixture");
    let result = twin_app::graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("batch.service")),
            ..GraphRequest::default()
        },
    )
    .expect("graph");
    let GraphResult::Service(result) = result else {
        panic!("service view")
    };
    assert_eq!(result.owned_processes.len(), 501);
    assert_eq!(result.evidence.len(), 500);
    for peer in &result.owned_processes {
        let pid = peer
            .id
            .strip_prefix("process:pid:")
            .expect("pid")
            .parse::<u32>()
            .expect("pid");
        assert_eq!(peer.label, format!("worker label {pid}"));
        assert_eq!(
            peer.observation_ids,
            vec![format!("00000000-0000-0000-0000-{pid:012}")]
        );
    }
    assert!(result
        .evidence
        .iter()
        .all(|line| line.source.starts_with("/proc/")
            && line.relationship.contains("service ownership inferred")));
    assert_eq!(store.count_nodes().expect("count"), 502);
}
