use twin_core::{EdgeClass, EdgeKind, GraphEdge, GraphNode, NodeId, NodeKind, TimestampNs};

#[test]
fn cgroup_and_service_nodes_preserve_first_seen() {
    let t0 = TimestampNs::new(100);
    let t1 = TimestampNs::new(200);
    let cgroup_first = GraphNode::cgroup("/system.slice/nginx.service", t0, None);
    let cgroup_second = GraphNode::cgroup("/system.slice/nginx.service", t1, Some(&cgroup_first));
    assert_eq!(cgroup_second.first_seen(), t0);
    assert_eq!(cgroup_second.last_seen(), t1);
    assert_eq!(cgroup_second.kind(), NodeKind::Cgroup);

    let service_first = GraphNode::service("nginx.service", t0, None);
    let service_second = GraphNode::service("nginx.service", t1, Some(&service_first));
    assert_eq!(service_second.first_seen(), t0);
    assert_eq!(service_second.label(), "nginx.service");
}

#[test]
fn cgroup_edges_use_expected_class_and_kind() {
    let t = TimestampNs::new(1);
    let process = NodeId::process(42);
    let cgroup = NodeId::cgroup("/system.slice/nginx.service");
    let service = NodeId::service("nginx.service");

    let in_cgroup = GraphEdge::observed_in_cgroup(&process, &cgroup, t, None);
    assert_eq!(in_cgroup.kind(), EdgeKind::InCgroup);
    assert_eq!(in_cgroup.class(), EdgeClass::Observed);

    let owns_proc = GraphEdge::inferred_service_owns_process(&service, &process, t, None);
    assert_eq!(owns_proc.kind(), EdgeKind::Owns);
    assert_eq!(owns_proc.class(), EdgeClass::Inferred);
    assert!(owns_proc
        .metadata()
        .as_str()
        .contains("systemd_cgroup_path"));
}

#[test]
fn tcp_port_and_listener_edges() {
    let t0 = TimestampNs::new(100);
    let t1 = TimestampNs::new(200);
    let port_first = GraphNode::tcp_port("127.0.0.1", 5432, t0, None).expect("port");
    let port_second = GraphNode::tcp_port("127.0.0.1", 5432, t1, Some(&port_first)).expect("port");
    assert_eq!(port_second.first_seen(), t0);
    assert_eq!(port_second.label(), "tcp:127.0.0.1:5432");

    let process = NodeId::process(721);
    let port = NodeId::port_tcp("127.0.0.1", 5432).expect("port id");
    let service = NodeId::service("postgresql.service");

    let listens = GraphEdge::observed_process_listens_on(&process, &port, t0, None);
    assert_eq!(listens.kind(), EdgeKind::ListensOn);
    assert_eq!(listens.class(), EdgeClass::Observed);

    let service_listens = GraphEdge::inferred_service_listens_on(&service, &port, t0, None);
    assert_eq!(service_listens.class(), EdgeClass::Inferred);
    assert!(service_listens
        .metadata()
        .as_str()
        .contains("service_owns_listening_process"));
}

#[test]
fn connection_and_dependency_edges() {
    let t0 = TimestampNs::new(100);
    let t1 = TimestampNs::new(200);
    let process = NodeId::process(8841);
    let port = NodeId::port_tcp("127.0.0.1", 5432).expect("port id");
    let django = NodeId::service("django.service");
    let postgres = NodeId::service("postgresql.service");

    let connects = GraphEdge::observed_process_connects_to(&process, &port, t0, None);
    assert_eq!(connects.kind(), EdgeKind::ConnectsTo);
    assert_eq!(connects.class(), EdgeClass::Observed);
    assert!(connects
        .metadata()
        .as_str()
        .contains("proc_tcp_established_inode_join"));

    let service_connects = GraphEdge::inferred_service_connects_to(&django, &port, t0, None);
    assert_eq!(service_connects.class(), EdgeClass::Inferred);

    let depends_first = GraphEdge::inferred_service_depends_on(&django, &postgres, t0, None);
    let depends_second =
        GraphEdge::inferred_service_depends_on(&django, &postgres, t1, Some(&depends_first));
    assert_eq!(depends_first.kind(), EdgeKind::DependsOn);
    assert_eq!(depends_first.class(), EdgeClass::Inferred);
    assert_eq!(depends_first.first_seen(), t0);
    assert_eq!(depends_second.last_seen(), t1);
    assert_eq!(depends_second.evidence_count(), 2);
    assert!(depends_first
        .metadata()
        .as_str()
        .contains("active_connection_to_listening_service"));

    let declared = GraphEdge::observed_service_depends_on_declared(
        &django,
        &postgres,
        t0,
        None,
        "Requires",
        "/usr/lib/systemd/system/django.service",
    );
    assert_eq!(declared.class(), EdgeClass::Observed);
    assert!(declared.metadata().as_str().contains("systemd_unit_file"));

    let escaped = GraphEdge::observed_service_depends_on_declared(
        &django,
        &postgres,
        t0,
        None,
        "Requires",
        r#"/usr/lib/systemd/system/with"quote.service"#,
    );
    assert!(escaped.metadata().as_str().contains(r#"with\"quote"#));
}
