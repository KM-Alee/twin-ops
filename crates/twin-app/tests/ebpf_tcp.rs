mod support;

use std::net::Ipv4Addr;

use twin_app::{
    connect_evidence_strength, emulate_in, graph_in, impact_in, init_in, EmulateActionRequest,
    EmulateRequest, GraphRequest, ImpactRequest, InitRequest, TcpIngestor,
};
use twin_core::{EvidenceLabel, GraphEdge, GraphNode, NodeId, TimestampNs};
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
