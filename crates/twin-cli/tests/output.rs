use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, EmulationImpact,
    EmulationImpactPathView, EmulationOverlayNode, EmulationResult, GraphEvidenceLine,
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
    assert!(text.contains("warnings (2 kinds, 6 events)"));
}

#[test]
fn impact_render_groups_unknown_kinds() {
    let mut result = sample_impact_result();
    result.unknowns = vec![
        ImpactUnknown {
            kind: "unmapped_listener_sockets".to_string(),
            detail: "3 unix listener sockets on /run/docker.sock".to_string(),
            source: None,
            weakens_evidence: true,
        },
        ImpactUnknown {
            kind: "unmapped_listener_sockets".to_string(),
            detail: "3 unix listener sockets on /var/run/metrics.sock".to_string(),
            source: None,
            weakens_evidence: true,
        },
    ];
    let text = output::impact::render(&result);
    assert!(text.contains("unmapped listeners"));
    assert!(text.contains("2 report(s)"));
    assert!(text.contains("weakens evidence"));
}

#[test]
fn graph_service_evidence_is_deduped() {
    use twin_app::{GraphEvidenceLine, GraphOwnedNode, GraphServiceResult};
    let result = GraphResult::Service(GraphServiceResult {
        service: twin_app::GraphNodeSummary {
            id: "service:docker.service".to_string(),
            label: "docker.service".to_string(),
        },
        owned_processes: vec![],
        owned_cgroups: vec![],
        listening_ports: vec![],
        listening_unix: vec![GraphOwnedNode {
            id: "unix:/run/docker.sock".to_string(),
            label: "/run/docker.sock".to_string(),
            edge_class: "inferred".to_string(),
            tag: None,
            observation_ids: vec![],
        }],
        connected_ports: vec![],
        connected_unix: vec![],
        dependencies: vec![],
        dependents: vec![],
        socket_activation: vec![],
        configured_dependents: vec![],
        configured_files: vec![],
        evidence: vec![
            GraphEvidenceLine {
                source: "/proc/net/unix:162".to_string(),
                statement: "inode 20562 listening on /run/docker.sock joined with /proc/1212/fd/7"
                    .to_string(),
                strength: "strong".to_string(),
                relationship: "process listener observed".to_string(),
            },
            GraphEvidenceLine {
                source: "/proc/net/unix:164".to_string(),
                statement: "inode 20562 listening on /run/docker.sock joined with /proc/1212/fd/7"
                    .to_string(),
                strength: "strong".to_string(),
                relationship: "process listener observed".to_string(),
            },
        ],
    });
    let text = output::graph::render(&result);
    assert_eq!(text.matches("inode 20562 listening").count(), 1);
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
        configured_files: vec![],
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
        configured_files: vec![],
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
fn impact_render_impact_paths_section_when_requested() {
    let mut result = sample_impact_result();
    result.paths_requested = true;
    result.impact_paths.push(twin_app::ImpactPath {
        terminal: twin_app::ImpactNodeSummary {
            id: "service:nginx.service".to_string(),
            label: "nginx.service".to_string(),
        },
        depth: 2,
        steps: vec![
            twin_app::ImpactPathStep {
                from: twin_app::ImpactNodeSummary {
                    id: "service:nginx.service".to_string(),
                    label: "nginx.service".to_string(),
                },
                edge_kind: "depends_on".to_string(),
                edge_class: "inferred".to_string(),
                to: twin_app::ImpactNodeSummary {
                    id: "service:django.service".to_string(),
                    label: "django.service".to_string(),
                },
                edge_id: "e1".to_string(),
            },
            twin_app::ImpactPathStep {
                from: twin_app::ImpactNodeSummary {
                    id: "service:django.service".to_string(),
                    label: "django.service".to_string(),
                },
                edge_kind: "depends_on".to_string(),
                edge_class: "inferred".to_string(),
                to: twin_app::ImpactNodeSummary {
                    id: "service:postgresql.service".to_string(),
                    label: "postgresql.service".to_string(),
                },
                edge_id: "e2".to_string(),
            },
        ],
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: false,
        cycle_note: None,
    });
    let text = output::impact::render(&result);
    assert!(text.contains("impact paths"));
    assert!(text
        .contains("service:nginx.service -> service:django.service -> service:postgresql.service"));
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
    assert!(text.contains("django.service"));
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
        impact_paths: vec![],
        paths_requested: false,
        max_depth: 4,
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
fn impact_json_includes_impact_paths_when_requested() {
    let mut result = sample_impact_result();
    result.paths_requested = true;
    result.impact_paths.push(twin_app::ImpactPath {
        terminal: twin_app::ImpactNodeSummary {
            id: "service:nginx.service".to_string(),
            label: "nginx.service".to_string(),
        },
        depth: 2,
        steps: vec![],
        evidence: vec![],
        is_cycle_capped: true,
        is_depth_capped: false,
        cycle_note: Some("cycle capped at service:nginx.service".to_string()),
    });
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(value["paths_requested"], true);
    assert_eq!(value["max_depth"], 4);
    let paths = value["impact_paths"]
        .as_array()
        .expect("impact_paths array");
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0]["terminal"]["id"], "service:nginx.service");
    assert_eq!(paths[0]["depth"], 2);
    assert_eq!(paths[0]["is_cycle_capped"], true);
    assert_eq!(paths[0]["is_depth_capped"], false);
    assert_eq!(
        paths[0]["cycle_note"],
        "cycle capped at service:nginx.service"
    );
}

#[test]
fn impact_render_depth_and_cycle_cap_notes() {
    let mut result = sample_impact_result();
    result.paths_requested = true;
    result.impact_paths.push(twin_app::ImpactPath {
        terminal: twin_app::ImpactNodeSummary {
            id: "service:api.service".to_string(),
            label: "api.service".to_string(),
        },
        depth: 4,
        steps: vec![],
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: true,
        cycle_note: None,
    });
    result.impact_paths.push(twin_app::ImpactPath {
        terminal: twin_app::ImpactNodeSummary {
            id: "service:worker.service".to_string(),
            label: "worker.service".to_string(),
        },
        depth: 2,
        steps: vec![],
        evidence: vec![],
        is_cycle_capped: true,
        is_depth_capped: false,
        cycle_note: Some("cycle capped at service:worker.service".to_string()),
    });
    let text = output::impact::render(&result);
    assert!(text.contains("depth capped at 4"));
    assert!(text.contains("cycle capped at service:worker.service"));
}

fn sample_emulation_result() -> EmulationResult {
    EmulationResult {
        action: "restart".to_string(),
        target: "service:postgresql.service".to_string(),
        target_label: "postgresql.service".to_string(),
        action_performed: false,
        safety_statement: "No action was performed.".to_string(),
        risk: twin_app::RiskAssessment {
            level: twin_core::RiskLevel::Medium,
            reasons: vec!["1 direct runtime dependent(s) may be interrupted".to_string()],
        },
        evidence_strength: twin_app::EvidenceStrengthView {
            score: 75,
            label: "strong".to_string(),
        },
        overlay: twin_app::EmulationOverlaySummary {
            unavailable_nodes: vec![
                EmulationOverlayNode {
                    id: "service:postgresql.service".to_string(),
                    label: "postgresql.service".to_string(),
                    reason: "temporarily unavailable".to_string(),
                },
                EmulationOverlayNode {
                    id: "port:tcp:127.0.0.1:5432".to_string(),
                    label: "127.0.0.1:5432".to_string(),
                    reason: "listener unavailable".to_string(),
                },
            ],
            interrupted_relationships: vec![],
        },
        transient_impacts: vec![EmulationImpact {
            id: "service:django.service".to_string(),
            label: "django.service".to_string(),
            statement: "django.service may lose dependency while postgresql.service restarts"
                .to_string(),
            path: "service:django.service depends_on service:postgresql.service".to_string(),
            evidence: vec!["active connection observed".to_string()],
        }],
        configured_impacts: vec![],
        runtime_impacts: vec![],
        restart_impacts: vec![],
        persistent_impacts: vec![],
        unknown_impacts: vec![],
        evidence_lines: vec![],
        general_safety_statement: "No action was performed.".to_string(),
        unknowns: vec![],
        impact_paths: vec![],
        paths_requested: false,
        max_depth: 4,
    }
}

fn sample_delete_emulation_result() -> EmulationResult {
    EmulationResult {
        action: "delete".to_string(),
        target: "file:/etc/nginx/nginx.conf".to_string(),
        target_label: "/etc/nginx/nginx.conf".to_string(),
        action_performed: false,
        safety_statement: "No file was deleted.".to_string(),
        general_safety_statement: "No action was performed.".to_string(),
        risk: twin_app::RiskAssessment {
            level: twin_core::RiskLevel::Critical,
            reasons: vec!["one service is configured by this file".to_string()],
        },
        evidence_strength: twin_app::EvidenceStrengthView {
            score: 70,
            label: "moderate".to_string(),
        },
        overlay: twin_app::EmulationOverlaySummary {
            unavailable_nodes: vec![EmulationOverlayNode {
                id: "file:/etc/nginx/nginx.conf".to_string(),
                label: "/etc/nginx/nginx.conf".to_string(),
                reason: "hypothetically deleted".to_string(),
            }],
            interrupted_relationships: vec![],
        },
        transient_impacts: vec![],
        configured_impacts: vec![],
        runtime_impacts: vec![EmulationImpact {
            id: "service:nginx.service".to_string(),
            label: "nginx.service".to_string(),
            statement: "Low immediate impact: running service is not assumed to reread /etc/nginx/nginx.conf immediately".to_string(),
            path: "service:nginx.service configured_by file:/etc/nginx/nginx.conf".to_string(),
            evidence: vec![],
        }],
        restart_impacts: vec![EmulationImpact {
            id: "service:nginx.service".to_string(),
            label: "nginx.service".to_string(),
            statement: "May fail to reload or restart without /etc/nginx/nginx.conf".to_string(),
            path: "service:nginx.service configured_by file:/etc/nginx/nginx.conf".to_string(),
            evidence: vec![],
        }],
        persistent_impacts: vec![EmulationImpact {
            id: "file:/etc/nginx/nginx.conf".to_string(),
            label: "/etc/nginx/nginx.conf".to_string(),
            statement: "Missing file remains a risk until restored".to_string(),
            path: String::new(),
            evidence: vec![],
        }],
        unknown_impacts: vec![],
        evidence_lines: vec![
            "known config path /etc/nginx/nginx.conf was discovered for nginx.service".to_string(),
        ],
        unknowns: vec![],
        impact_paths: vec![],
        paths_requested: false,
        max_depth: 0,
    }
}

#[test]
fn emulate_restart_output_has_overlay_transient_impact_and_safety_statement() {
    let text = output::emulate::render(&sample_emulation_result());
    assert!(text.contains("twin emulate restart"));
    assert!(text.contains("overlay"));
    assert!(text.contains("transient impact"));
    assert!(text.contains("No action was performed."));
    assert!(text.contains("django.service"));
    assert!(text.contains("direct-only scoring"));
    assert!(text.contains("path"));
    assert!(text.contains("evidence"));
}

#[test]
fn emulate_restart_output_omits_empty_sections_cleanly() {
    let mut result = sample_emulation_result();
    result.transient_impacts.clear();
    result.configured_impacts.clear();
    result.overlay.unavailable_nodes.clear();
    result.unknowns.clear();
    let text = output::emulate::render(&result);
    assert!(!text.contains("transient impact"));
    assert!(!text.contains("configured context"));
    assert!(!text.contains("unknowns"));
    assert!(text.contains("No action was performed."));
}

#[test]
fn emulate_delete_output_has_runtime_restart_persistent_and_safety() {
    let text = output::emulate::render(&sample_delete_emulation_result());
    assert!(text.contains("Emulation: delete"));
    assert!(text.contains("runtime impact"));
    assert!(text.contains("restart impact"));
    assert!(text.contains("persistent impact"));
    assert!(text.contains("No file was deleted."));
    assert!(text.contains("No action was performed."));
}

#[test]
fn emulate_delete_json_contains_action_performed_false() {
    let json = output::json::render(&sample_delete_emulation_result()).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(value["action_performed"], false);
    assert_eq!(value["action"], "delete");
    assert!(value.get("runtime_impacts").is_some());
    assert!(value.get("restart_impacts").is_some());
}

#[test]
fn emulate_restart_json_contains_action_performed_false() {
    let json = output::json::render(&sample_emulation_result()).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(value["action_performed"], false);
    assert_eq!(value["action"], "restart");
    assert!(value.get("overlay").is_some());
}

#[test]
fn emulate_restart_json_includes_impact_paths_when_requested() {
    let mut result = sample_emulation_result();
    result.paths_requested = true;
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:nginx.service".to_string(),
        terminal_label: "nginx.service".to_string(),
        depth: 2,
        path_chain: "service:nginx.service -> service:django.service -> service:postgresql.service"
            .to_string(),
        evidence: vec!["Observed: active connection".to_string()],
        is_cycle_capped: false,
        is_depth_capped: true,
        cycle_note: None,
    });
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(value["paths_requested"], true);
    let paths = value["impact_paths"]
        .as_array()
        .expect("impact_paths array");
    assert_eq!(paths.len(), 1);
    assert_eq!(paths[0]["terminal_id"], "service:nginx.service");
    assert_eq!(paths[0]["depth"], 2);
    assert_eq!(paths[0]["is_depth_capped"], true);
    assert_eq!(
        paths[0]["path_chain"],
        "service:nginx.service -> service:django.service -> service:postgresql.service"
    );
}

#[test]
fn emulate_restart_output_renders_impact_paths_section_when_requested() {
    let mut result = sample_emulation_result();
    result.paths_requested = true;
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:nginx.service".to_string(),
        terminal_label: "nginx.service".to_string(),
        depth: 2,
        path_chain: "service:nginx.service -> service:django.service -> service:postgresql.service"
            .to_string(),
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: false,
        cycle_note: None,
    });
    let text = output::emulate::render(&result);
    assert!(text.contains("impact paths"));
    assert!(text.contains("nginx.service"));
    assert!(text.contains("transitive path evidence"));
    assert!(!text.contains("direct-only scoring"));
}

#[test]
fn emulate_restart_summary_dedupes_transitive_dependents_by_terminal_id() {
    let mut result = sample_emulation_result();
    result.paths_requested = true;
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:nginx.service".to_string(),
        terminal_label: "nginx.service".to_string(),
        depth: 2,
        path_chain: "service:nginx.service -> service:django.service".to_string(),
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: false,
        cycle_note: None,
    });
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:nginx.service".to_string(),
        terminal_label: "nginx.service".to_string(),
        depth: 3,
        path_chain: "service:nginx.service -> service:api.service -> service:django.service"
            .to_string(),
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: false,
        cycle_note: None,
    });
    let text = output::emulate::render(&result);
    assert!(text.contains("1 transitive dependent(s)"));
    assert!(!text.contains("2 transitive dependent(s)"));
}

#[test]
fn emulate_restart_output_renders_depth_and_cycle_cap_notes() {
    let mut result = sample_emulation_result();
    result.paths_requested = true;
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:api.service".to_string(),
        terminal_label: "api.service".to_string(),
        depth: 4,
        path_chain: "service:api.service -> service:django.service".to_string(),
        evidence: vec![],
        is_cycle_capped: false,
        is_depth_capped: true,
        cycle_note: None,
    });
    result.impact_paths.push(EmulationImpactPathView {
        terminal_id: "service:worker.service".to_string(),
        terminal_label: "worker.service".to_string(),
        depth: 2,
        path_chain: "service:worker.service -> service:django.service".to_string(),
        evidence: vec![],
        is_cycle_capped: true,
        is_depth_capped: false,
        cycle_note: Some("cycle capped at service:worker.service".to_string()),
    });
    let text = output::emulate::render(&result);
    assert!(text.contains("depth capped at 4"));
    assert!(text.contains("cycle capped at service:worker.service"));
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
