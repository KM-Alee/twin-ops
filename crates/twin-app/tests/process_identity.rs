mod support;

use std::{fs, path::Path};
use twin_app::{InitRequest, ScanRequest};
use twin_core::{GraphEdge, GraphNode, NodeId, TimestampNs};
use twin_store::{EdgeRow, NodeRow, Store};

fn stat(start: &str) -> String {
    let mut fields = vec!["0"; 50];
    fields[0] = "S";
    fields[1] = "1";
    fields[19] = start;
    format!("42 (api) {}", fields.join(" "))
}

fn fixture(root: &Path) {
    support::write_proc_fixture(root, 42, &stat("100"), "Uid:\t0\t0\t0\t0\n", b"api\0", None);
    for table in ["tcp", "tcp6", "udp", "udp6"] {
        support::write_tcp_table(root, table, "sl local_address rem_address st tx_queue rx_queue tr tm->when retrnsmt uid timeout inode\n");
    }
    support::write_unix_table(root, "Num RefCount Protocol Flags Type St Inode Path\n");
}

#[test]
fn known_identity_survives_unknown_gap_and_replacement_closes_stale_evidence() {
    for change_boot in [false, true] {
        let home = support::IsolatedHome::new();
        twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
        let root = home.layout.data_dir.join("proc");
        fixture(&root);
        twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
        let mut store = Store::open(&home.layout.db_file()).unwrap();
        let id = NodeId::process(42);
        let first = store.get_node(id.as_str()).unwrap().unwrap();
        let endpoint = GraphNode::tcp_port("192.0.2.10", 443, TimestampNs::now(), None).unwrap();
        store.upsert_node(&NodeRow::from(&endpoint)).unwrap();
        let mut edge = EdgeRow::from(&GraphEdge::observed_process_connects_to(
            &id,
            endpoint.id(),
            TimestampNs::now(),
            None,
        ));
        edge.state = "stale".into();
        edge.metadata_json = "{\"source\":\"ebpf_tcp\"}".into();
        store.upsert_edge(&edge).unwrap();
        let observation = store
            .list_observations_by_kind("process_seen")
            .unwrap()
            .remove(0);
        store
            .link_edge_observation(&edge.id, &observation.id, "direct")
            .unwrap();
        let boot = root.join("sys/kernel/random/boot_id");
        fs::remove_file(&boot).unwrap();
        fs::write(root.join("42/stat"), stat("unknown")).unwrap();
        twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
        let unknown = store.get_node(id.as_str()).unwrap().unwrap();
        let metadata: serde_json::Value = serde_json::from_str(&unknown.metadata_json).unwrap();
        assert_eq!(metadata["start_ticks"], 100);
        assert_eq!(metadata["start_identity_known"], false);
        assert_eq!(metadata["boot_identity_known"], false);
        assert_eq!(unknown.first_seen_ns, first.first_seen_ns);
        fs::write(
            boot,
            if change_boot {
                "00000000-0000-0000-0000-000000000002"
            } else {
                "00000000-0000-0000-0000-000000000001"
            },
        )
        .unwrap();
        fs::write(
            root.join("42/stat"),
            stat(if change_boot { "100" } else { "200" }),
        )
        .unwrap();
        twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
        let next = store.get_node(id.as_str()).unwrap().unwrap();
        assert!(next.first_seen_ns > first.first_seen_ns);
        let closed = store.get_edge(&edge.id).unwrap().unwrap();
        assert_eq!(closed.state, "gone");
        assert_eq!(closed.evidence_count, 0);
        assert_eq!(closed.first_seen_ns, next.first_seen_ns);
        assert!(store
            .list_observations_for_edge(&edge.id)
            .unwrap()
            .is_empty());
    }
}

#[test]
fn gone_process_reappearance_starts_new_lifetime_even_with_same_identity() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, InitRequest::default()).unwrap();
    let root = home.layout.data_dir.join("proc");
    fixture(&root);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let mut store = Store::open(&home.layout.db_file()).unwrap();
    let id = NodeId::process(42);
    let first = store.get_node(id.as_str()).unwrap().unwrap();
    store
        .mark_nodes_missing(&[id.to_string()], TimestampNs::now().as_i64(), false, None)
        .unwrap();
    twin_app::scan_in(&home.layout, ScanRequest::default(), &root).unwrap();
    let reappeared = store.get_node(id.as_str()).unwrap().unwrap();
    assert_eq!(reappeared.state, "active");
    assert!(reappeared.first_seen_ns > first.first_seen_ns);
}
