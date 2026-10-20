mod support;

use std::net::Ipv4Addr;

use twin_app::{
    connect_evidence_strength, emulate_in, graph_in, impact_in, init_in, EmulateActionRequest,
    EmulateRequest, GraphRequest, ImpactRequest, InitRequest, ScanRequest, TcpIngestor,
};
use twin_core::{EvidenceLabel, GraphEdge, GraphNode, NodeId, NodeState, TimestampNs};
use twin_ebpf::{TcpAccept, TcpBind, TcpConnect, TcpEvent, TcpRateLimits};
use twin_observation::ObservationKind;
use twin_store::Store;

fn ipv4(octets: [u8; 4]) -> std::net::IpAddr {
    std::net::IpAddr::V4(Ipv4Addr::from(octets))
}

fn connect(pid: u32, timestamp: u64) -> TcpEvent {
    TcpEvent::Connect(TcpConnect::new(
        pid,
        9,
        ipv4([10, 0, 0, 1]),
        ipv4([10, 0, 0, 8]),
        5432,
        timestamp,
    ))
}

fn open_store(home: &support::IsolatedHome) -> Store {
    Store::open(&home.layout.db_file()).expect("store")
}

#[test]
fn rate_limit_stores_one_dropped_row() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = open_store(&home);
    let mut ingest = TcpIngestor::new(TcpRateLimits {
        per_pid: 1,
        per_endpoint: 8,
        global_limit: 8,
        ..TcpRateLimits::session()
    });
    let mut stored = 0usize;
    for index in 0..4 {
        if ingest
            .push(&mut store, &connect(4421, index))
            .expect("push")
            .is_some()
        {
            stored += 1;
        }
    }
    ingest.sync_drops(&mut store, 4).expect("drops");
    assert_eq!(stored, 1);
    assert_eq!(ingest.dropped(), 3);
    let connects = store
        .list_observations_by_kind(&ObservationKind::EbpfConnect.to_string())
        .expect("connects");
    let dropped = store
        .list_observations_by_kind(&ObservationKind::EbpfDroppedEvents.to_string())
        .expect("dropped");
    assert_eq!(connects.len(), 1);
    assert_eq!(dropped.len(), 1);
    let meta: serde_json::Value =
        serde_json::from_str(&dropped[0].metadata_json).expect("metadata");
    assert_eq!(meta["source"], "ebpf_tcp");
    assert_eq!(meta["count"], 3);
    assert_eq!(store.count_observations().expect("count"), 2);
}

#[test]
fn repeated_connect_infers_service_edge_and_drop_weakens_it() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    {
        let mut store = open_store(&home);
        seed_django_postgres(&mut store);
        let mut ingest = TcpIngestor::new(TcpRateLimits {
            per_pid: 8,
            per_endpoint: 8,
            global_limit: 8,
            ..TcpRateLimits::session()
        });
        for timestamp in [1, 2] {
            assert!(ingest
                .push(&mut store, &connect(4421, timestamp))
                .expect("push")
                .is_some());
        }
        assert!(ingest
            .push(
                &mut store,
                &TcpEvent::Accept(TcpAccept::new(
                    50,
                    1,
                    ipv4([10, 0, 0, 8]),
                    5432,
                    ipv4([10, 0, 0, 1]),
                    40000,
                    3,
                )),
            )
            .expect("accept")
            .is_some());
        assert!(ingest
            .push(
                &mut store,
                &TcpEvent::Bind(TcpBind::new(50, 1, ipv4([10, 0, 0, 8]), 5432, 4)),
            )
            .expect("bind")
            .is_some());
    }

    let view = service_graph(&home, true);
    let twin_app::GraphResult::Service(service) = view else {
        panic!("expected service");
    };
    assert_eq!(service.runtime_dependencies.len(), 1);
    let dependency = &service.runtime_dependencies[0];
    assert_eq!(dependency.from_id, "service:django.service");
    assert_eq!(dependency.to_id, "service:postgresql.service");
    assert_eq!(dependency.relationship, "connects_to");
    assert_eq!(dependency.evidence_label, "very_strong");
    assert!(dependency
        .reasons
        .iter()
        .any(|reason| reason == "eBPF observed 2 connect events"));
    assert!(dependency
        .reasons
        .iter()
        .any(|reason| reason == "socket inode mapping confirmed listener ownership"));

    let store = open_store(&home);
    let edges = store.list_edges_by_kind("connects_to").expect("edges");
    assert!(edges.iter().any(|edge| {
        edge.from_node_id == "process:pid:4421" && edge.to_node_id == "port:tcp:10.0.0.8:5432"
    }));
    assert!(edges.iter().any(|edge| {
        edge.from_node_id == "service:django.service"
            && edge.to_node_id == "service:postgresql.service"
    }));
    assert_eq!(
        store
            .list_observations_by_kind(&ObservationKind::EbpfAccept.to_string())
            .expect("accept")
            .len(),
        1
    );
    assert_eq!(
        store
            .list_observations_by_kind(&ObservationKind::EbpfBind.to_string())
            .expect("bind")
            .len(),
        1
    );
    drop(store);

    {
        let mut store = open_store(&home);
        let mut ingest = TcpIngestor::new(TcpRateLimits {
            per_pid: 0,
            per_endpoint: 0,
            global_limit: 0,
            ..TcpRateLimits::session()
        });
        assert!(ingest
            .push(&mut store, &connect(4421, 9))
            .expect("drop")
            .is_none());
        assert!(ingest
            .push(&mut store, &connect(4421, 10))
            .expect("drop")
            .is_none());
        ingest.sync_drops(&mut store, 10).expect("sync");
        assert_eq!(
            store
                .list_observations_by_kind(&ObservationKind::EbpfConnect.to_string())
                .expect("connects")
                .len(),
            2
        );
        assert_eq!(
            store
                .list_observations_by_kind(&ObservationKind::EbpfDroppedEvents.to_string())
                .expect("dropped")
                .len(),
            1
        );
    }

    let view = service_graph(&home, true);
    let twin_app::GraphResult::Service(service) = view else {
        panic!("expected service");
    };
    assert_eq!(service.runtime_dependencies[0].evidence_label, "strong");

    let hidden = service_graph(&home, false);
    let twin_app::GraphResult::Service(service) = hidden else {
        panic!("expected service");
    };
    assert!(service.runtime_dependencies.is_empty());

    let impact = impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::service("postgresql.service")),
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(impact.direct_dependents.iter().any(|dependent| {
        dependent.id == "service:django.service"
            && dependent
                .evidence
                .iter()
                .any(|line| line.statement.contains("eBPF observed"))
    }));

    let emulation = emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::service("postgresql.service")),
                target_query: None,
                show_paths: false,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert!(emulation.transient_impacts.iter().any(|impact| {
        impact
            .evidence
            .iter()
            .any(|line| line.contains("eBPF observed"))
    }));
}

#[test]
fn connect_without_service_ownership_stays_on_the_process() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = open_store(&home);
    let seen = TimestampNs::new(1);
    store
        .upsert_node_typed(&GraphNode::process(4421, "django", seen, None))
        .expect("process");
    let mut ingest = TcpIngestor::new(TcpRateLimits::session());
    ingest.push(&mut store, &connect(4421, 1)).expect("push");
    let edges = store.list_edges_by_kind("connects_to").expect("edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0].from_node_id, "process:pid:4421");
    assert_eq!(edges[0].to_node_id, "port:tcp:10.0.0.8:5432");
}

#[test]
fn evidence_labels_follow_the_existing_scale() {
    assert_eq!(
        connect_evidence_strength(0, true, false, false).label(),
        EvidenceLabel::Strong
    );
    assert_eq!(
        connect_evidence_strength(0, false, true, false).label(),
        EvidenceLabel::Moderate
    );
    assert_eq!(
        connect_evidence_strength(2, false, false, false).label(),
        EvidenceLabel::VeryStrong
    );
    assert_eq!(
        connect_evidence_strength(2, true, false, true).label(),
        EvidenceLabel::Strong
    );
    assert_eq!(
        connect_evidence_strength(0, true, false, true).label(),
        EvidenceLabel::Strong
    );
    assert_eq!(
        connect_evidence_strength(2, false, false, false).score(),
        86
    );
    assert_eq!(connect_evidence_strength(0, true, false, false).score(), 75);
    assert_eq!(connect_evidence_strength(0, false, true, false).score(), 45);
    assert_eq!(connect_evidence_strength(2, true, false, true).score(), 75);
    assert_eq!(connect_evidence_strength(0, true, false, true).score(), 75);
}

fn service_graph(home: &support::IsolatedHome, show_evidence: bool) -> twin_app::GraphResult {
    graph_in(
        &home.layout,
        GraphRequest {
            target: Some(NodeId::service("django.service")),
            show_evidence,
            ..GraphRequest::default()
        },
    )
    .expect("graph")
}

fn seed_django_postgres(store: &mut Store) {
    let seen = TimestampNs::new(1);
    let django = NodeId::service("django.service");
    let postgres = NodeId::service("postgresql.service");
    let process = NodeId::process(4421);
    let port = NodeId::port_tcp("10.0.0.8", 5432).expect("port");
    store
        .upsert_node_typed(&GraphNode::service("django.service", seen, None))
        .expect("django");
    store
        .upsert_node_typed(&GraphNode::service("postgresql.service", seen, None))
        .expect("postgres");
    store
        .upsert_node_typed(&GraphNode::process(4421, "django", seen, None))
        .expect("process");
    store
        .upsert_node_typed(&GraphNode::tcp_port("10.0.0.8", 5432, seen, None).expect("port node"))
        .expect("port");
    store
        .upsert_edge_typed(&GraphEdge::inferred_service_owns(
            &django, &process, seen, None,
        ))
        .expect("owns");
    store
        .upsert_edge_typed(&GraphEdge::inferred_service_listens_on(
            &postgres, &port, seen, None,
        ))
        .expect("listens");
}

#[test]
fn host_scan_keeps_ebpf_evidence_while_source_is_active() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    {
        let mut store = open_store(&home);
        seed_django_postgres(&mut store);
        let mut ingest = TcpIngestor::new(TcpRateLimits {
            per_pid: 8,
            per_endpoint: 8,
            global_limit: 8,
            ..TcpRateLimits::session()
        });
        assert!(ingest
            .push(&mut store, &connect(4421, 1))
            .expect("push")
            .is_some());
    }

    let proc = home.layout.data_dir.join("fixture-proc-ebpf");
    support::write_proc_fixture(
        &proc,
        4421,
        "4421 (django) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/django\0",
        None,
    );
    std::fs::create_dir_all(proc.join("4421/fd")).expect("readable empty fd directory");
    for table in ["tcp", "tcp6", "udp", "udp6"] {
        support::write_tcp_table(&proc, table, "sl local_address rem_address st\n");
    }
    support::write_unix_table(&proc, "Num RefCount Protocol Flags Type St Inode Path\n");
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("host scan");

    let store = open_store(&home);
    let edge = store
        .get_edge("process:pid:4421|connects_to|port:tcp:10.0.0.8:5432")
        .expect("edge")
        .expect("connect edge");
    assert_eq!(
        edge.state, "active",
        "stored eBPF evidence must survive a host scan while the process is alive"
    );
    let port_id = NodeId::port_tcp("10.0.0.8", 5432).expect("port");
    let port = store
        .get_node_typed(&port_id)
        .expect("node")
        .expect("port node");
    assert_eq!(port.state(), NodeState::Gone);

    let mut store = open_store(&home);
    let mut ingest = TcpIngestor::new(TcpRateLimits {
        per_pid: 8,
        per_endpoint: 8,
        global_limit: 8,
        ..TcpRateLimits::session()
    });
    assert!(ingest
        .push(&mut store, &connect(4421, 2))
        .expect("push")
        .is_some());
    let port = store
        .get_node_typed(&port_id)
        .expect("node")
        .expect("port node");
    assert_eq!(
        port.state(),
        NodeState::Active,
        "a new observation reactivates the endpoint"
    );
}

#[test]
fn ebpf_only_connect_records_node_and_edge_history() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = open_store(&home);
    let mut ingest = TcpIngestor::new(TcpRateLimits {
        per_pid: 8,
        per_endpoint: 8,
        global_limit: 8,
        ..TcpRateLimits::session()
    });
    ingest.push(&mut store, &connect(4421, 1)).unwrap();
    ingest.push(&mut store, &connect(4421, 2)).unwrap();
    let nodes = store.list_node_history_since(0).unwrap();
    assert_eq!(nodes.len(), 2);
    assert!(nodes.iter().all(|row| row.change_kind == "new"));
    let edges = store.list_edge_history_since(0).unwrap();
    assert_eq!(edges.len(), 1, "a repeated sighting is not a change");
    assert_eq!(edges[0].change_kind, "new");
    let current = store.get_edge(&edges[0].edge_id).unwrap().expect("edge");
    assert_eq!(current.evidence_count, 2);
}

#[test]
fn kernel_cgroup_identity_attributes_new_pids_and_preserves_observation_links() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = open_store(&home);
    let seen = TimestampNs::new(1);
    let service = NodeId::service("worker.service");
    let cgroup = NodeId::cgroup("/system.slice/worker.service");
    store
        .upsert_node_typed(&GraphNode::service("worker.service", seen, None))
        .expect("service");
    store
        .upsert_node_typed(&GraphNode::identified_cgroup(
            "/system.slice/worker.service",
            Some(9),
            seen,
            None,
        ))
        .expect("cgroup");
    store
        .upsert_edge_typed(&GraphEdge::inferred_service_owns_cgroup(
            &service, &cgroup, seen, None,
        ))
        .expect("owner");
    let mut ingest = TcpIngestor::new(TcpRateLimits::session());
    ingest.push(&mut store, &connect(501, 2)).expect("connect");
    for id in [
        "process:pid:501|in_cgroup|cgroup:/system.slice/worker.service",
        "service:worker.service|owns|process:pid:501",
        "service:worker.service|connects_to|port:tcp:10.0.0.8:5432",
    ] {
        assert!(store.get_edge(id).expect("edge").is_some(), "{id}");
        assert!(
            !store
                .list_observations_for_edge(id)
                .expect("links")
                .is_empty(),
            "{id}"
        );
    }
    // A missing kernel identity must not borrow a previous PID's service.
    let event = TcpEvent::Connect(TcpConnect::new(
        502,
        99,
        ipv4([10, 0, 0, 1]),
        ipv4([10, 0, 0, 8]),
        5432,
        3,
    ));
    ingest.push(&mut store, &event).expect("unmapped");
    assert!(store
        .list_active_edges_to("process:pid:502")
        .expect("edges")
        .is_empty());
}

#[test]
fn live_event_does_not_join_foreign_namespace_to_host_listener_or_reused_pid() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).unwrap();
    let mut store = open_store(&home);
    seed_django_postgres(&mut store);
    let TcpEvent::Connect(event) = connect(4421, 10) else {
        unreachable!()
    };
    let mut bytes = twin_ebpf::encode_connect(&event).to_vec();
    bytes.extend_from_slice(&2u64.to_le_bytes());
    bytes.extend_from_slice(&300u64.to_le_bytes());
    let event = twin_ebpf::decode_tcp_event(&bytes).unwrap();
    let mut ingest = TcpIngestor::new(TcpRateLimits::session());
    assert!(ingest.push(&mut store, &event).unwrap().is_some());
    assert!(store
        .get_node("port:tcp:10.0.0.8:5432:netns:2")
        .unwrap()
        .is_some());
    assert!(!store
        .list_edges_by_kind("connects_to")
        .unwrap()
        .iter()
        .any(|edge| edge.from_node_id == "service:django.service"
            && edge.to_node_id == "service:postgresql.service"));
    let process = store.get_node("process:pid:4421").unwrap().unwrap();
    assert!(process.metadata_json.contains("300"));
    bytes[64..72].copy_from_slice(&200u64.to_le_bytes());
    let stale = twin_ebpf::decode_tcp_event(&bytes).unwrap();
    assert!(ingest.push(&mut store, &stale).unwrap().is_none());
    assert_eq!(ingest.dropped(), 1);
    assert_eq!(
        store
            .get_node("process:pid:4421")
            .unwrap()
            .unwrap()
            .metadata_json,
        process.metadata_json
    );
}
