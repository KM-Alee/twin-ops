use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, GraphEvidenceLine,
    GraphListResult, GraphNodeResult, GraphNodeSummary, GraphOwnedNode, GraphResult,
    GraphServiceResult, ImpactDependent, ImpactEvidenceLine, ImpactResult, InitResult,
    PermissionMode, ScanResult, ScanWarning,
};
use twin_cli::output;
use twin_core::NodeKind;
use twin_store::LATEST_VERSION;

fn sample_init_result() -> InitResult {
    InitResult {
        config_path: PathBuf::from("/tmp/config.toml"),
        db_path: PathBuf::from("/tmp/twin.db"),
        log_path: PathBuf::from("/tmp/twin.log"),
        config_created: true,
        config_updated: false,
        db_created: true,
        schema_version: LATEST_VERSION,
    }
}

#[test]
fn init_render_created() {
    let text = output::init::render(&sample_init_result());
    assert!(text.contains("twin init"));
    assert!(text.contains("created"));
}

#[test]
fn init_render_unchanged() {
    let result = InitResult {
        config_created: false,
        config_updated: false,
        db_created: false,
        ..sample_init_result()
    };
    let text = output::init::render(&result);
    assert!(text.contains("unchanged"));
}

#[test]
fn doctor_render_sections() {
    let result = DoctorResult {
        core: DoctorCore {
            cli_ok: true,
            config_found: false,
            config_path: Some(PathBuf::from("/tmp/config.toml")),
        },
        database: DoctorDatabase {
            initialized: false,
            schema_version: None,
            db_path: Some(PathBuf::from("/tmp/twin.db")),
            wal_mode: None,
        },
        permissions: DoctorPermissions {
            mode: PermissionMode::Unprivileged,
            proc_accessible: true,
            readable_process_count: Some(10),
            restricted_process_count: Some(2),
        },
    };
    let text = output::doctor::render(&result);
    assert!(text.contains("twin doctor"));
    assert!(text.contains("not initialized"));
    assert!(text.contains("├──"));
}

#[test]
fn scan_render_counts() {
    let result = ScanResult {
        started_at_ns: 1,
        ended_at_ns: 2,
        process_count: 143,
        parent_edge_count: 128,
        cgroup_count: 24,
        service_count: 17,
        in_cgroup_edge_count: 141,
        service_owns_edge_count: 62,
        tcp_listener_count: 6,
        port_count: 6,
        process_listens_on_edge_count: 6,
        service_listens_on_edge_count: 4,
        unmapped_listener_socket_count: 0,
        tcp_connection_count: 3,
        process_connects_to_edge_count: 3,
        service_connects_to_edge_count: 2,
        service_depends_on_edge_count: 1,
        unmapped_active_socket_count: 0,
        socket_owner_inode_count: 6,
        observation_count: 500,
        warning_count: 0,
        warnings: vec![],
        warning_details: vec![],
    };
    let text = output::scan::render(&result);
    assert!(text.contains("twin scan"));
    assert!(text.contains("143"));
    assert!(text.contains("parent-of"));
    assert!(text.contains("in-cgroup"));
    assert!(text.contains("service-owns"));
    assert!(text.contains("ports"));
    assert!(text.contains("process-listens-on"));
    assert!(text.contains("active-connections"));
    assert!(text.contains("service-depends-on"));
}

#[test]
fn scan_render_warning_aggregation() {
    let result = ScanResult {
        started_at_ns: 1,
        ended_at_ns: 2,
        process_count: 1,
        parent_edge_count: 0,
        cgroup_count: 0,
        service_count: 0,
        in_cgroup_edge_count: 0,
        service_owns_edge_count: 0,
        tcp_listener_count: 0,
        port_count: 0,
        process_listens_on_edge_count: 0,
        service_listens_on_edge_count: 0,
        unmapped_listener_socket_count: 0,
        tcp_connection_count: 0,
        process_connects_to_edge_count: 0,
        service_connects_to_edge_count: 0,
        service_depends_on_edge_count: 0,
        unmapped_active_socket_count: 0,
        socket_owner_inode_count: 0,
        observation_count: 1,
        warning_count: 6,
        warnings: vec![
            ScanWarning {
                kind: "vanished_processes".to_string(),
                count: 4,
            },
            ScanWarning {
                kind: "exe_unreadable".to_string(),
                count: 2,
            },
        ],
        warning_details: vec![],
    };
    let text = output::scan::render(&result);
    assert!(text.contains("4 disappeared during scan"));
    assert!(text.contains("2 unreadable exe links"));
}

#[test]
fn graph_list_render_stable() {
    let result = GraphResult::List(GraphListResult {
        kind: NodeKind::Process,
        nodes: vec![
            twin_app::GraphNodeSummary {
                id: "process:pid:1".to_string(),
                label: "systemd".to_string(),
            },
            twin_app::GraphNodeSummary {
                id: "process:pid:42".to_string(),
                label: "nginx".to_string(),
            },
        ],
        parent_edges: vec![twin_app::GraphParentEdge {
            parent_id: "process:pid:1".to_string(),
            child_id: "process:pid:42".to_string(),
            child_label: "nginx".to_string(),
        }],
    });
    let text = output::graph::render(&result);
    assert!(text.contains("twin graph"));
    assert!(text.contains("Process tree"));
    assert!(text.contains("└──"));
    assert!(text.contains("systemd"));
    assert!(text.contains("nginx"));
}

#[test]
fn graph_service_render_inferred_ownership() {
    let result = GraphResult::service(GraphServiceResult {
        service: GraphNodeSummary {
            id: "service:nginx.service".to_string(),
            label: "nginx.service".to_string(),
        },
        owned_processes: vec![GraphOwnedNode {
            id: "process:pid:1432".to_string(),
            label: "nginx".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec!["obs-1".to_string()],
        }],
        owned_cgroups: vec![GraphOwnedNode {
            id: "cgroup:/system.slice/nginx.service".to_string(),
            label: "/system.slice/nginx.service".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec![],
        }],
        listening_ports: vec![GraphOwnedNode {
            id: "port:tcp:127.0.0.1:5432".to_string(),
            label: "tcp:127.0.0.1:5432".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec![],
        }],
        connected_ports: vec![GraphOwnedNode {
            id: "port:tcp:127.0.0.1:8000".to_string(),
            label: "tcp:127.0.0.1:8000".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec![],
        }],
        dependencies: vec![GraphOwnedNode {
            id: "service:postgresql.service".to_string(),
            label: "postgresql.service".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec![],
        }],
        dependents: vec![],
        evidence: vec![GraphEvidenceLine {
            source: "/proc/1432/cgroup".to_string(),
            statement: "contains /system.slice/nginx.service".to_string(),
            strength: "high".to_string(),
            relationship: "service ownership inferred from systemd cgroup path".to_string(),
        }],
    });
    let text = output::graph::render(&result);
    assert!(text.contains("owns (inferred)"));
    assert!(text.contains("listens on (inferred)"));
    assert!(text.contains("connects to (inferred from active TCP)"));
    assert!(text.contains("depends on (inferred)"));
    assert!(text.contains("/proc/1432/cgroup"));
    assert!(text.contains("service neighborhood"));
}

#[test]
fn graph_node_render_parents_children() {
    let result = GraphResult::Node(GraphNodeResult {
        node: twin_app::GraphNodeSummary {
            id: "process:pid:42".to_string(),
            label: "nginx".to_string(),
        },
        incoming: vec![twin_app::GraphEdgeSummary {
            id: "e1".to_string(),
            kind: "parent_of".to_string(),
            peer_id: "process:pid:1".to_string(),
            peer_label: "systemd".to_string(),
            observation_ids: vec![],
        }],
        outgoing: vec![],
        evidence_refs: vec!["/proc/42/stat ppid=1".to_string()],
    });
    let text = output::graph::render(&result);
    assert!(text.contains("systemd"));
    assert!(text.contains("└──"));
    assert!(text.contains("no children"));
    assert!(text.contains("evidence"));
    assert!(text.contains("/proc/42/stat ppid=1"));
}

#[test]
fn scan_json_includes_warning_details_field() {
    use twin_app::ScanWarningDetail;
    let result = ScanResult {
        started_at_ns: 1,
        ended_at_ns: 2,
        process_count: 1,
        parent_edge_count: 0,
        cgroup_count: 0,
        service_count: 0,
        in_cgroup_edge_count: 0,
        service_owns_edge_count: 0,
        tcp_listener_count: 0,
        port_count: 0,
        process_listens_on_edge_count: 0,
        service_listens_on_edge_count: 0,
        unmapped_listener_socket_count: 0,
        tcp_connection_count: 0,
        process_connects_to_edge_count: 0,
        service_connects_to_edge_count: 0,
        service_depends_on_edge_count: 0,
        unmapped_active_socket_count: 0,
        socket_owner_inode_count: 0,
        observation_count: 1,
        warning_count: 1,
        warnings: vec![ScanWarning {
            kind: "vanished_processes".to_string(),
            count: 1,
        }],
        warning_details: vec![ScanWarningDetail {
            kind: "vanished".to_string(),
            path: "/proc/99/stat".to_string(),
            detail: "not found".to_string(),
        }],
    };
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    let details = value
        .get("warning_details")
        .and_then(|v| v.as_array())
        .expect("warning_details array");
    assert_eq!(details.len(), 1);
}

#[test]
fn impact_render_direct_dependents_and_evidence() {
    let result = ImpactResult {
        target: "service:postgresql.service".to_string(),
        target_label: "postgresql.service".to_string(),
        direct_dependents: vec![ImpactDependent {
            id: "service:django.service".to_string(),
            label: "django.service".to_string(),
            relationship: "depends_on".to_string(),
            edge_class: "inferred".to_string(),
            observation_ids: vec!["obs-1".to_string()],
        }],
        evidence: vec![ImpactEvidenceLine {
            source: "/proc/net/tcp:2".to_string(),
            statement: "inode 456 established from 127.0.0.1:50122 to 127.0.0.1:5432 joined with /proc/8841/fd/12"
                .to_string(),
            relationship:
                "process connection observed; service dependency inferred from listener match"
                    .to_string(),
        }],
        unknowns: vec![],
    };
    let text = output::impact::render(&result);
    assert!(text.contains("twin impact"));
    assert!(text.contains("direct dependents"));
    assert!(text.contains("service:django.service"));
    assert!(text.contains("depends_on"));
    assert!(text.contains("/proc/net/tcp:2"));
    assert!(!text.contains("risk"));
}

#[test]
fn init_json_includes_schema_version() {
    let result = sample_init_result();
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_i64()),
        Some(LATEST_VERSION)
    );
}
