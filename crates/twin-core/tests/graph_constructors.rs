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
