use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, GraphListResult, GraphNodeResult,
    GraphResult, InitResult, PermissionMode, ScanResult, ScanWarning,
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
    assert!(text.contains("Twin init: Created"));
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
    assert!(text.contains("Already exists"));
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
    assert!(text.contains("Twin Doctor"));
    assert!(text.contains("not initialized"));
}

#[test]
fn scan_render_counts() {
    let result = ScanResult {
        started_at_ns: 1,
        ended_at_ns: 2,
        process_count: 143,
        parent_edge_count: 128,
        observation_count: 500,
        warning_count: 0,
        warnings: vec![],
        warning_details: vec![],
    };
    let text = output::scan::render(&result);
    assert!(text.contains("processes: 143"));
    assert!(text.contains("parent-child: 128"));
}

#[test]
fn scan_render_warning_aggregation() {
    let result = ScanResult {
        started_at_ns: 1,
        ended_at_ns: 2,
        process_count: 1,
        parent_edge_count: 0,
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
    assert!(text.contains("4 processes disappeared"));
    assert!(text.contains("2 process exe links unreadable"));
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
    assert!(text.contains("Process tree"));
    assert!(text.contains("└──"));
    assert!(text.contains("systemd"));
    assert!(text.contains("nginx"));
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
fn init_json_includes_schema_version() {
    let result = sample_init_result();
    let json = output::json::render(&result).expect("json");
    let value: serde_json::Value = serde_json::from_str(&json).expect("parse");
    assert_eq!(
        value.get("schema_version").and_then(|v| v.as_i64()),
        Some(LATEST_VERSION)
    );
}
