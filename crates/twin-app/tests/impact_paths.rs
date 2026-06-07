mod support;

use twin_app::{impact_in, ImpactRequest, InitRequest, ScanRequest};
use twin_core::RiskLevel;
use twin_store::Store;

use support::{write_proc_fixture_with_cgroup, write_socket_fd, write_tcp_table, IsolatedHome};

const TCP_HEADER: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode";

fn init_scanned_postgres(home: &IsolatedHome) {
    twin_app::init_in(&home.layout, InitRequest::default()).expect("init");
    let proc = home.layout.data_dir.join("fixture-proc-impact-paths");
    std::fs::create_dir_all(&proc).expect("proc");
    write_proc_fixture_with_cgroup(
        &proc,
        721,
        "721 (postgres) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t999\t999\t999\t999\n",
        b"/usr/bin/postgres\0",
        None,
        Some("0::/system.slice/postgresql.service\n"),
    );
    write_proc_fixture_with_cgroup(
        &proc,
        8841,
        "8841 (gunicorn) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"/usr/bin/gunicorn\0",
        None,
        Some("0::/system.slice/django.service\n"),
    );
    write_tcp_table(
        &proc,
        "tcp",
        &format!(
            "{TCP_HEADER}\n   0: 0100007F:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:C3CA 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 456 1 0000000000000000 100 0 0 10 0"
        ),
    );
    write_socket_fd(&proc, 721, 8, 12345);
    write_socket_fd(&proc, 8841, 12, 456);
    twin_app::scan_in(&home.layout, ScanRequest::default(), &proc).expect("scan");
}

fn upsert_service(store: &mut Store, id: &str, label: &str) {
    store
        .upsert_node(&twin_store::NodeRow {
            id: id.to_string(),
            kind: "service".to_string(),
            label: label.to_string(),
            state: "active".to_string(),
            first_seen_ns: 1,
            last_seen_ns: 1,
            valid_from_ns: 1,
            valid_to_ns: None,
            metadata_json: "{}".to_string(),
        })
        .expect("node");
}

fn upsert_depends_on(store: &mut Store, from: &str, to: &str, class: &str) {
    store
        .upsert_edge(&twin_store::EdgeRow {
            id: format!("{from}|depends_on|{to}"),
            from_node_id: from.to_string(),
            to_node_id: to.to_string(),
            kind: "depends_on".to_string(),
            class: class.to_string(),
            state: "active".to_string(),
            evidence_score: 0,
            evidence_label: "weak".to_string(),
            evidence_count: 0,
            first_seen_ns: 1,
            last_seen_ns: 1,
            metadata_json: "{}".to_string(),
        })
        .expect("edge");
}

fn impact_paths(home: &IsolatedHome, max_depth: usize) -> twin_app::ImpactResult {
    impact_in(
        &home.layout,
        ImpactRequest {
            target_query: Some("postgresql".to_string()),
            show_paths: true,
            max_depth,
            ..ImpactRequest::default()
        },
    )
    .expect("impact")
}

#[test]
fn impact_paths_direct_dependent_depth_one() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let result = impact_paths(&home, 4);
    assert_eq!(result.impact_paths.len(), 1);
    assert_eq!(result.impact_paths[0].depth, 1);
    assert_eq!(result.impact_paths[0].terminal.label, "django.service");
}

#[test]
fn impact_paths_transitive_chain() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_service(&mut store, "service:nginx.service", "nginx.service");
    upsert_depends_on(
        &mut store,
        "service:nginx.service",
        "service:django.service",
        "inferred",
    );
    let result = impact_paths(&home, 4);
    let path = result
        .impact_paths
        .iter()
        .find(|p| p.terminal.label == "nginx.service")
        .expect("nginx path");
    assert_eq!(path.depth, 2);
    assert_eq!(path.steps.len(), 2);
    assert_eq!(path.steps[0].from.label, "nginx.service");
    assert_eq!(path.steps[1].to.label, "postgresql.service");
}

#[test]
fn impact_paths_branching_sorted() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_service(&mut store, "service:worker.service", "worker.service");
    upsert_depends_on(
        &mut store,
        "service:worker.service",
        "service:postgresql.service",
        "inferred",
    );
    let result = impact_paths(&home, 4);
    assert_eq!(result.impact_paths.len(), 2);
    assert_eq!(result.impact_paths[0].terminal.label, "django.service");
    assert_eq!(result.impact_paths[1].terminal.label, "worker.service");
}

#[test]
fn impact_paths_cycle_capped() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_depends_on(
        &mut store,
        "service:postgresql.service",
        "service:django.service",
        "inferred",
    );
    let result = impact_paths(&home, 4);
    assert!(result.impact_paths.iter().any(|p| p.is_cycle_capped));
}

#[test]
fn impact_paths_max_depth_caps() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_service(&mut store, "service:nginx.service", "nginx.service");
    upsert_service(&mut store, "service:edge.service", "edge.service");
    upsert_depends_on(
        &mut store,
        "service:nginx.service",
        "service:django.service",
        "inferred",
    );
    upsert_depends_on(
        &mut store,
        "service:edge.service",
        "service:nginx.service",
        "inferred",
    );
    let result = impact_paths(&home, 2);
    let capped = result
        .impact_paths
        .iter()
        .find(|p| p.terminal.label == "nginx.service")
        .expect("nginx depth-capped path");
    assert!(capped.is_depth_capped);
    assert_eq!(capped.depth, 2);
    assert!(!result
        .impact_paths
        .iter()
        .any(|p| p.terminal.label == "edge.service"));
}

#[test]
fn impact_paths_missing_evidence_unknown() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_service(&mut store, "service:worker.service", "worker.service");
    upsert_depends_on(
        &mut store,
        "service:worker.service",
        "service:postgresql.service",
        "inferred",
    );
    let result = impact_paths(&home, 4);
    assert!(!result.impact_paths.is_empty());
    assert!(result.unknowns.iter().any(|u| u.kind == "missing_evidence"));
}

#[test]
fn impact_scoring_transitive_raises_risk_reason() {
    let home = IsolatedHome::new();
    init_scanned_postgres(&home);
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    upsert_service(&mut store, "service:nginx.service", "nginx.service");
    upsert_depends_on(
        &mut store,
        "service:nginx.service",
        "service:django.service",
        "inferred",
    );
    let result = impact_paths(&home, 4);
    assert!(result
        .risk
        .reasons
        .iter()
        .any(|r| r.contains("transitive runtime dependent")));
    assert_ne!(result.risk.level, RiskLevel::Low);
}
