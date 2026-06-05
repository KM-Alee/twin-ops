use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, GraphEvidenceLine,
    GraphListResult, GraphNodeResult, GraphNodeSummary, GraphOwnedNode, GraphResult,
    GraphServiceResult, ImpactDependent, ImpactEvidenceLine, ImpactResult, ImpactUnknown,
    InitResult, PermissionMode, ScanQuality, ScanQualityAssessment, ScanResult, ScanWarning,
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
        scan_quality: None,
        scan_quality_error: None,
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
        tcp_connection_count: 3,
        process_connects_to_edge_count: 3,
        service_connects_to_edge_count: 2,
        service_depends_on_edge_count: 1,
        socket_owner_inode_count: 6,
        observation_count: 500,
        ..Default::default()
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
    assert!(text.contains("unix-sockets"));
    assert!(text.contains("dbus-available"));
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
        declared_depends_on_edge_count: 0,
        systemd_unit_count: 0,
        enable_depends_on_edge_count: 0,
        dbus_depends_on_edge_count: 0,
        dbus_available: false,
        unix_listener_count: 0,
        unix_socket_count: 0,
        process_listens_on_unix_edge_count: 0,
        service_listens_on_unix_edge_count: 0,
        unmapped_unix_listener_count: 0,
        unix_connection_count: 0,
        process_connects_to_unix_edge_count: 0,
        service_connects_to_unix_edge_count: 0,
        service_depends_on_unix_edge_count: 0,
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
        ..Default::default()
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
fn doctor_render_scan_quality_degraded() {
    let result = DoctorResult {
        core: DoctorCore {
            cli_ok: true,
            config_found: true,
            config_path: Some(PathBuf::from("/tmp/config.toml")),
        },
        database: DoctorDatabase {
            initialized: true,
            schema_version: Some(LATEST_VERSION),
            db_path: Some(PathBuf::from("/tmp/twin.db")),
            wal_mode: Some(true),
        },
        permissions: DoctorPermissions {
            mode: PermissionMode::Privileged,
            proc_accessible: true,
            readable_process_count: Some(100),
            restricted_process_count: Some(0),
        },
        scan_quality: Some(ScanQualityAssessment {
            quality: ScanQuality::Degraded,
            reasons: vec!["50 fd directories hidden".to_string()],
            impact_reliable: false,
            ephemeral_capture_recommended: false,
        }),
        scan_quality_error: None,
    };
    let text = output::doctor::render(&result);
    assert!(text.contains("scan quality"));
    assert!(text.contains("DEGRADED"));
    assert!(text.contains("50 fd directories hidden"));
}

#[test]
fn doctor_render_ephemeral_capture_recommendation() {
    let result = DoctorResult {
        core: DoctorCore {
            cli_ok: true,
            config_found: true,
            config_path: None,
        },
        database: DoctorDatabase {
            initialized: true,
            schema_version: Some(1),
            db_path: None,
            wal_mode: Some(true),
        },
        permissions: DoctorPermissions {
            mode: PermissionMode::Privileged,
            proc_accessible: true,
            readable_process_count: Some(10),
            restricted_process_count: Some(0),
        },
        scan_quality: Some(ScanQualityAssessment {
            quality: ScanQuality::Good,
            reasons: vec![],
            impact_reliable: true,
            ephemeral_capture_recommended: true,
        }),
        scan_quality_error: None,
    };
    let text = output::doctor::render(&result);
    assert!(text.contains("ephemeral capture"));
    assert!(text.contains("--samples 5"));
}

#[test]
fn graph_service_render_declared_dependency() {
    let result = GraphResult::service(GraphServiceResult {
        service: GraphNodeSummary {
            id: "service:docker.service".to_string(),
            label: "docker.service".to_string(),
        },
        owned_processes: vec![],
        owned_cgroups: vec![],
        listening_ports: vec![],
        listening_unix: vec![],
        connected_ports: vec![],
        connected_unix: vec![],
        dependencies: vec![GraphOwnedNode {
            id: "service:containerd.service".to_string(),
            label: "containerd.service".to_string(),
            edge_class: "observed".to_string(),
            tag: Some("declared".to_string()),
            observation_ids: vec![],
        }],
        dependents: vec![],
        socket_activation: vec![],
        configured_dependents: vec![],
        evidence: vec![],
    });
    let text = output::graph::render(&result);
    assert!(text.contains("depends on (declared)"));
    assert!(text.contains("containerd.service"));
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
            tag: None,
            observation_ids: vec!["obs-1".to_string()],
        }],
        owned_cgroups: vec![GraphOwnedNode {
            id: "cgroup:/system.slice/nginx.service".to_string(),
            label: "/system.slice/nginx.service".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        listening_ports: vec![GraphOwnedNode {
            id: "port:tcp:127.0.0.1:5432".to_string(),
            label: "tcp:127.0.0.1:5432".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        connected_ports: vec![GraphOwnedNode {
            id: "port:tcp:127.0.0.1:8000".to_string(),
            label: "tcp:127.0.0.1:8000".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        listening_unix: vec![],
        connected_unix: vec![],
        dependencies: vec![GraphOwnedNode {
            id: "service:postgresql.service".to_string(),
            label: "postgresql.service".to_string(),
            edge_class: "inferred".to_string(),
            tag: Some("runtime".to_string()),
            observation_ids: vec![],
        }],
        dependents: vec![],
        socket_activation: vec![],
        configured_dependents: vec![],
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
    assert!(text.contains("depends on (runtime inferred)"));
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
fn graph_unix_socket_render_listeners_and_callers() {
    let result = GraphResult::unix_socket(
        GraphNodeSummary {
            id: "unix:/run/dbus/system_bus_socket".to_string(),
            label: "/run/dbus/system_bus_socket".to_string(),
        },
        vec![],
        vec![GraphOwnedNode {
            id: "service:dbus-broker.service".to_string(),
            label: "dbus-broker.service".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        vec![],
        vec![GraphOwnedNode {
            id: "service:pipewire.service".to_string(),
            label: "pipewire.service".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        vec![],
    );
    let text = output::graph::render(&result);
    assert!(text.contains("unix socket neighborhood"));
    assert!(text.contains("dbus-broker.service"));
    assert!(text.contains("pipewire.service"));
    assert!(text.contains("callers"));
}

#[test]
fn scan_json_includes_warning_details_field() {
    use twin_app::ScanWarningDetail;
    let result = ScanResult {
        process_count: 1,
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
        ..Default::default()
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
fn impact_render_configured_dependent_and_scan_health() {
    let mut result = sample_impact_result();
    let configured = result.direct_dependents.remove(0);
    let mut configured = configured;
    configured.impact_kind = "configured".to_string();
    configured.edge_class = "observed".to_string();
    configured.reason = "Requires=postgresql.service in unit file".to_string();
    result.configured_dependents.push(configured);
    result.unknowns.push(ImpactUnknown {
        kind: "scan_health".to_string(),
        detail: "scan quality degraded — runtime TCP dependency detection may be incomplete"
            .to_string(),
        source: Some("latest scan".to_string()),
        weakens_evidence: false,
    });
    let text = output::impact::render(&result);
    assert!(text.contains("configured"));
    assert!(text.contains("scan health"));
    assert!(text.contains("runtime TCP"));
}

#[test]
fn impact_render_direct_dependents_and_evidence() {
    let result = sample_impact_result();
    let text = output::impact::render(&result);
    assert!(text.contains("twin impact"));
    assert!(text.contains("impact report"));
    assert!(text.contains("risk"));
    assert!(text.contains("evidence strength"));
    assert!(text.contains("direct dependents"));
    assert!(text.contains("service:django.service"));
    assert!(text.contains("depends_on"));
    assert!(text.contains("/proc/net/tcp:2"));
}

fn sample_impact_result() -> ImpactResult {
    ImpactResult {
        target: "service:postgresql.service".to_string(),
        target_label: "postgresql.service".to_string(),
        risk: twin_app::RiskAssessment {
            level: twin_core::RiskLevel::High,
            reasons: vec!["1 direct dependent is known".to_string()],
        },
        evidence_strength: twin_app::EvidenceStrengthView {
            score: 75,
            label: "strong".to_string(),
        },
        configured_dependents: vec![],
        direct_dependents: vec![ImpactDependent {
            id: "service:django.service".to_string(),
            label: "django.service".to_string(),
            relationship: "depends_on".to_string(),
            edge_class: "inferred".to_string(),
            impact_kind: "runtime".to_string(),
            reason: "active connection to 127.0.0.1:5432".to_string(),
            path: vec![],
            evidence: vec![],
            observation_ids: vec!["obs-1".to_string()],
        }],
        listener_owners: vec![],
        evidence: vec![ImpactEvidenceLine {
            source: "/proc/net/tcp:2".to_string(),
            statement: "Observed: inode 456 established from 127.0.0.1:50122 to 127.0.0.1:5432"
                .to_string(),
            relationship:
                "Inferred: service dependency inferred from active connection and listener match"
                    .to_string(),
            strength: "strong".to_string(),
            observation_id: Some("obs-1".to_string()),
        }],
        unknowns: vec![],
    }
}

#[test]
fn impact_json_includes_risk_and_unknowns() {
    let result = sample_impact_result();
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert!(value.get("risk").is_some());
    assert!(value.get("evidence_strength").is_some());
    assert!(value.get("listener_owners").is_some());
    assert!(value.get("unknowns").is_some());
    assert!(value
        .get("direct_dependents")
        .and_then(|v| v.as_array())
        .is_some());
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
