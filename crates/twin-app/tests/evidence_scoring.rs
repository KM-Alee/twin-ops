mod support;

use twin_app::{
    emulate_in, impact_in, init_in, scan_in, EmulateActionRequest, EmulateRequest, ImpactRequest,
    InitRequest, ScanRequest,
};
use twin_core::{GraphNode, NodeId, TimestampNs};
use twin_store::{CollectorRunRow, Store};

#[test]
fn coverage_unknowns_appear_on_impact_and_emulate_without_crashing() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("store");
    let seen = TimestampNs::new(1_000);
    store
        .upsert_node_typed(&GraphNode::service("nginx.service", seen, None))
        .expect("node");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: "proc_process".to_string(),
            started_at_ns: 1_000,
            ended_at_ns: 2_000,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 11,
            error_message: None,
            metadata_json: concat!(
                r#"{"readable_processes":10,"permission_denied":8,"#,
                r#""fd_permission_denied":0,"cgroup_permission_denied":0,"#,
                r#""socket_unmapped":1,"active_socket_unmapped":2,"#,
                r#""unix_socket_unmapped":0,"unix_connection_unmapped":0}"#
            )
            .to_string(),
        })
        .expect("run");
    drop(store);

    let impact = impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::service("nginx.service")),
            show_evidence: true,
            ..ImpactRequest::default()
        },
    )
    .expect("impact");
    assert!(impact
        .unknowns
        .iter()
        .any(|unknown| unknown.detail == "8 processes hidden due to permissions"));
    assert!(impact
        .unknowns
        .iter()
        .any(|unknown| unknown.detail == "3 sockets could not be mapped"));
    assert!(impact.show_evidence);
    assert!(!impact.evidence_reasons.is_empty());

    let emulation = emulate_in(
        &home.layout,
        EmulateRequest {
            show_evidence: true,
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::service("nginx.service")),
                target_query: None,
                show_paths: false,
                max_depth: 4,
            },
            ..EmulateRequest::default()
        },
    )
    .expect("emulate");
    assert!(emulation
        .unknowns
        .iter()
        .any(|unknown| unknown.detail == "8 processes hidden due to permissions"));
    assert!(emulation.show_evidence);
    assert!(!emulation.evidence_reasons.is_empty());
}

#[test]
fn scan_reports_readable_processes_in_coverage() {
    let _env = support::lock_scan_env();
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");
    let proc_root = home.layout.data_dir.join("empty-proc");
    std::fs::create_dir_all(&proc_root).expect("proc");
    let result = scan_in(&home.layout, ScanRequest::default(), &proc_root).expect("scan");
    assert_eq!(result.coverage.readable_processes, result.process_count);
    let again = scan_in(&home.layout, ScanRequest::default(), &proc_root).expect("scan");
    assert_eq!(
        again.coverage.readable_processes,
        result.coverage.readable_processes
    );
    assert_eq!(
        again.coverage.restricted_processes,
        result.coverage.restricted_processes
    );
}
