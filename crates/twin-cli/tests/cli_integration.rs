mod support;

use std::path::PathBuf;

use support::{
    init_and_scan, stderr_utf8, stdout_utf8, twin_bin, write_ambiguous_services_fixture,
    write_malformed_cgroup_fixture, write_non_systemd_cgroup_fixture, write_slice5_fixture,
    TwinHome,
};
use twin_store::Store;

fn xdg_db_path(home: &TwinHome) -> PathBuf {
    home.home_path()
        .join(".local")
        .join("share")
        .join("twin")
        .join("twin.db")
}

#[test]
fn twin_binary_is_available() {
    assert!(twin_bin().exists());
}

#[test]
fn init_creates_db_and_config_with_title() {
    let home = TwinHome::new();
    let out = home.run_without_proc(&["init"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("twin init"));
    assert!(text.contains("created"));
    assert!(text.contains("layout"));
    assert!(xdg_db_path(&home).exists());
    let cfg = home
        .home_path()
        .join(".config")
        .join("twin")
        .join("config.toml");
    assert!(cfg.exists());
}

#[test]
fn init_second_run_is_idempotent() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let second = home.run_without_proc(&["init"]);
    assert!(second.status.success());
    let text = stdout_utf8(&second);
    assert!(text.contains("twin init"));
    assert!(text.contains("unchanged"));
}

#[test]
fn doctor_before_init_reports_not_initialized() {
    let home = TwinHome::new();
    let out = home.run_without_proc(&["doctor"]);
    assert!(out.status.success());
    let text = stdout_utf8(&out);
    assert!(text.contains("twin doctor"));
    assert!(text.contains("not initialized"));
    assert!(text.contains("run twin init first"));
}

#[test]
fn doctor_after_init_reports_ready() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let out = home.run_without_proc(&["doctor"]);
    assert!(out.status.success());
    let text = stdout_utf8(&out);
    assert!(text.contains("ready to scan"));
    assert!(text.contains("initialized"));
}

#[test]
fn scan_without_init_fails_cleanly() {
    let home = TwinHome::new();
    write_slice5_fixture(&home.proc_root);
    let out = home.run(&["scan"]);
    assert!(!out.status.success());
    let err = stderr_utf8(&out);
    assert!(
        err.contains("not initialized") || err.contains("DatabaseNotInitialized"),
        "unexpected stderr: {err}"
    );
}

#[test]
fn scan_human_output_includes_slice5_sections() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["scan"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("twin scan"));
    assert!(text.contains("persisted"));
    assert!(text.contains("cgroups"));
    assert!(text.contains("services"));
    assert!(text.contains("in-cgroup"));
    assert!(text.contains("service-owns"));
    assert!(text.contains("2")); // process count in status or tree
}

#[test]
fn scan_json_matches_fixture_counts() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["--json", "scan"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("scan json");
    assert_eq!(value["process_count"], 2);
    assert_eq!(value["service_count"], 1);
    assert_eq!(value["cgroup_count"], 1);
    assert!(value["in_cgroup_edge_count"].as_u64().unwrap() >= 1);
    assert!(value["service_owns_edge_count"].as_u64().unwrap() >= 2);
    assert!(value["observation_count"].as_u64().unwrap() > 0);
}

#[test]
fn graph_default_lists_process_tree() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("twin graph"));
    assert!(text.contains("Process tree"));
    assert!(text.contains("systemd"));
    assert!(text.contains("nginx"));
}

#[test]
fn graph_process_pid_neighborhood() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "process:pid:42"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("process neighborhood"));
    assert!(text.contains("systemd"));
    assert!(text.contains("nginx"));
}

#[test]
fn graph_service_id_shows_owns_and_cgroup_evidence() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "service:nginx.service"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("service neighborhood"));
    assert!(text.contains("owns (inferred)"));
    assert!(text.contains("/proc/42/cgroup"));
    assert!(text.contains("nginx"));
}

#[test]
fn graph_nginx_shorthand_resolves_service() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "nginx"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("service:nginx.service"));
    assert!(text.contains("owns (inferred)"));
}

#[test]
fn graph_kind_service_lists_services() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "--kind", "service"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("Service list"));
    assert!(text.contains("nginx.service"));
}

#[test]
fn graph_json_service_view_has_expected_shape() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["--json", "graph", "nginx"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("graph json");
    assert_eq!(value["service"]["id"], "service:nginx.service");
    assert!(value["owned_processes"].as_array().unwrap().len() >= 1);
    assert!(value["evidence"].as_array().unwrap().len() >= 1);
    let source = value["evidence"][0]["source"].as_str().expect("source");
    assert!(source.contains("/proc/42/cgroup"));
}

#[test]
fn graph_before_init_fails() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let db = xdg_db_path(&home);
    std::fs::remove_file(&db).expect("remove db");
    let out = home.run(&["graph"]);
    assert!(!out.status.success());
    assert!(stderr_utf8(&out).contains("not initialized"));
}

#[test]
fn graph_unknown_service_fails() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "missing.service"]);
    assert!(!out.status.success());
    let err = stderr_utf8(&out);
    assert!(
        err.contains("not found") || err.contains("ServiceNotFound"),
        "unexpected: {err}"
    );
}

#[test]
fn scan_twice_updates_last_seen() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let first = home.run(&["scan"]);
    assert!(first.status.success());
    std::thread::sleep(std::time::Duration::from_millis(2));
    let second = home.run(&["scan"]);
    assert!(second.status.success());
    let store = Store::open(&xdg_db_path(&home)).expect("open");
    let node = store.get_node("process:pid:42").expect("get").expect("row");
    assert!(node.last_seen_ns >= node.first_seen_ns);
}

#[test]
fn graph_ambiguous_service_query_fails() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    write_ambiguous_services_fixture(&home.proc_root);
    assert!(home.run(&["scan"]).status.success());
    let out = home.run(&["graph", "ng"]);
    assert!(!out.status.success());
    let err = stderr_utf8(&out);
    assert!(
        err.contains("ambiguous") || err.contains("AmbiguousService"),
        "unexpected: {err}"
    );
    assert!(err.contains("nginx") || err.contains("ang"));
}

#[test]
fn init_json_includes_schema_version() {
    let home = TwinHome::new();
    let out = home.run_without_proc(&["--json", "init"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("init json");
    assert!(value.get("schema_version").is_some());
    assert!(value.get("db_path").is_some());
}

#[test]
fn doctor_json_after_init() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let out = home.run_without_proc(&["--json", "doctor"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("doctor json");
    assert_eq!(value["database"]["initialized"], true);
}

#[test]
fn scan_json_counts_match_slice5_fixture_exactly() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["--json", "scan"]);
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("scan json");
    assert_eq!(value["process_count"], 2);
    assert_eq!(value["parent_edge_count"], 1);
    assert_eq!(value["cgroup_count"], 1);
    assert_eq!(value["service_count"], 1);
    assert_eq!(value["in_cgroup_edge_count"], 1);
    assert_eq!(value["service_owns_edge_count"], 2);
}

#[test]
fn scan_persists_observed_in_cgroup_and_inferred_owns_in_db() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let store = Store::open(&xdg_db_path(&home)).expect("open");
    let in_cgroup = store.list_edges_by_kind("in_cgroup").expect("in_cgroup");
    assert_eq!(in_cgroup.len(), 1);
    assert_eq!(in_cgroup[0].class, "observed");
    assert_eq!(in_cgroup[0].from_node_id, "process:pid:42");
    assert_eq!(
        in_cgroup[0].to_node_id,
        "cgroup:/system.slice/nginx.service"
    );

    let owns = store.list_edges_by_kind("owns").expect("owns");
    assert_eq!(owns.len(), 2);
    assert!(owns.iter().all(|e| e.class == "inferred"));
    assert!(owns
        .iter()
        .all(|e| e.from_node_id == "service:nginx.service"));

    let links = store
        .list_observations_for_edge(&in_cgroup[0].id)
        .expect("edge obs");
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].1, "direct");
}

#[test]
fn scan_non_systemd_cgroup_has_no_service_nodes() {
    let home = TwinHome::new();
    assert!(home.run(&["init"]).status.success());
    write_non_systemd_cgroup_fixture(&home.proc_root);
    let scan = home.run(&["scan"]);
    assert!(scan.status.success(), "{}", stderr_utf8(&scan));
    let out = home.run(&["--json", "scan"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("scan json");
    assert_eq!(value["process_count"], 1);
    assert_eq!(value["service_count"], 0);
    assert_eq!(value["cgroup_count"], 1);
    assert_eq!(value["service_owns_edge_count"], 0);
    assert_eq!(value["in_cgroup_edge_count"], 1);
}

#[test]
fn scan_malformed_cgroup_line_surfaces_in_json_warning_details() {
    let home = TwinHome::new();
    assert!(home.run(&["init"]).status.success());
    write_malformed_cgroup_fixture(&home.proc_root);
    let out = home.run(&["--json", "scan"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("scan json");
    assert!(value["warning_count"].as_u64().unwrap() >= 1);
    let details = value["warning_details"]
        .as_array()
        .expect("warning_details");
    assert!(
        details.iter().any(|d| {
            d["kind"].as_str() == Some("cgroup_malformed")
                || d.get("path")
                    .and_then(|p| p.as_str())
                    .is_some_and(|p| p.contains("cgroup"))
        }),
        "expected cgroup_malformed detail, got {details:?}"
    );
    assert_eq!(value["service_count"], 1);
    assert_eq!(value["process_count"], 1);
}

#[test]
fn graph_pid_shorthand_numeric() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "42"]);
    assert!(out.status.success(), "{}", stderr_utf8(&out));
    let text = stdout_utf8(&out);
    assert!(text.contains("process neighborhood"));
    assert!(text.contains("process:pid:42"));
}

#[test]
fn graph_json_process_list_shape() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["--json", "graph"]);
    assert!(out.status.success());
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("graph json");
    assert!(
        value["kind"].as_str() == Some("process") || value["kind"].as_str() == Some("Process"),
        "kind: {:?}",
        value["kind"]
    );
    let nodes = value["nodes"].as_array().expect("nodes");
    assert_eq!(nodes.len(), 2);
    let edges = value["parent_edges"].as_array().expect("parent_edges");
    assert_eq!(edges.len(), 1);
    assert_eq!(edges[0]["child_id"], "process:pid:42");
}

#[test]
fn graph_service_json_owned_nodes_are_inferred() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["--json", "graph", "service:nginx.service"]);
    let value: serde_json::Value =
        serde_json::from_str(stdout_utf8(&out).trim()).expect("graph json");
    let processes = value["owned_processes"]
        .as_array()
        .expect("owned_processes");
    assert!(processes.iter().all(|n| n["edge_class"] == "inferred"));
    let evidence = value["evidence"].as_array().expect("evidence");
    assert!(evidence.iter().any(|e| {
        e["relationship"]
            .as_str()
            .is_some_and(|r| r.contains("inferred"))
    }));
}

#[test]
fn scan_twice_json_preserves_first_seen_in_db() {
    let home = TwinHome::new();
    init_and_scan(&home);
    assert!(home.run(&["scan"]).status.success());
    let store = Store::open(&xdg_db_path(&home)).expect("open");
    let first = store
        .get_node("service:nginx.service")
        .expect("get")
        .expect("row");
    std::thread::sleep(std::time::Duration::from_millis(2));
    assert!(home.run(&["scan"]).status.success());
    let second = store
        .get_node("service:nginx.service")
        .expect("get")
        .expect("row");
    assert_eq!(second.first_seen_ns, first.first_seen_ns);
    assert!(second.last_seen_ns >= first.last_seen_ns);
}

#[test]
fn graph_invalid_target_exits_nonzero() {
    let home = TwinHome::new();
    init_and_scan(&home);
    let out = home.run(&["graph", "not-a-valid-node-id!!!"]);
    assert!(!out.status.success());
    assert!(
        stderr_utf8(&out).contains("invalid graph target") || stderr_utf8(&out).contains("Error")
    );
}
