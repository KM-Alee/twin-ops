mod support;

use twin_app::{BriefRequest, BriefResult, PreflightVerdict};

use support::IsolatedHome;

fn brief(home: &IsolatedHome, proc: &std::path::Path, target: Option<&str>) -> BriefResult {
    twin_app::brief_in(
        &home.layout,
        &BriefRequest {
            target: target.map(str::to_string),
            config_override: None,
            max_graph_age_secs: u64::MAX,
            changes_since: "24h".into(),
        },
        proc,
    )
    .expect("brief")
}

fn section<'a>(result: &'a BriefResult, title: &str) -> &'a twin_app::BriefSection {
    result
        .sections
        .iter()
        .find(|section| section.title == title)
        .unwrap_or_else(|| panic!("missing section {title}: {:#?}", result.sections))
}

#[test]
fn service_brief_answers_state_listeners_dependents_and_restart_risk_in_one_call() {
    let home = IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    let result = brief(&home, &proc, Some("postgresql"));
    assert_eq!(result.kind, "service");
    assert_eq!(result.target.as_deref(), Some("service:postgresql.service"));
    assert!(section(&result, "listens on")
        .items
        .iter()
        .any(|item| item.label.contains("5432")));
    assert!(section(&result, "depended on by")
        .items
        .iter()
        .any(|item| item.label == "django.service"));
    let restart = result.restart.as_ref().expect("restart prediction");
    assert!(restart.verdict >= PreflightVerdict::Caution);
    assert!(restart.summary.contains("django.service"));
    assert!(result
        .next_steps
        .iter()
        .any(|step| step.starts_with("twin preflight")));
}

#[test]
fn host_brief_lists_network_exposure_and_hub_services() {
    let home = IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    std::fs::create_dir_all(proc.join("sys/kernel")).expect("sys");
    std::fs::write(proc.join("sys/kernel/hostname"), "db-1\n").expect("hostname");
    let result = brief(&home, &proc, None);
    assert_eq!(result.kind, "host");
    assert_eq!(result.title, "db-1");
    assert!(section(&result, "exposed to the network")
        .items
        .iter()
        .any(|item| item.label == "postgresql.service" && item.detail.contains("0.0.0.0:5432")));
    assert!(section(&result, "most depended-on services")
        .items
        .iter()
        .any(|item| item.label == "postgresql.service"));
}

#[test]
fn port_brief_reports_exposure_listeners_and_clients() {
    let home = IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    let result = brief(&home, &proc, Some("port:tcp:0.0.0.0:5432"));
    assert_eq!(result.kind, "port");
    assert!(result
        .facts
        .iter()
        .any(|fact| fact.value == "reachable from the network"));
    assert!(section(&result, "listeners")
        .items
        .iter()
        .any(|item| item.label == "postgresql.service"));
}

#[test]
fn unknown_target_and_missing_store_are_errors() {
    let home = IsolatedHome::new();
    let proc = home.layout.data_dir.join("proc");
    assert!(twin_app::brief_in(
        &home.layout,
        &BriefRequest {
            target: None,
            config_override: None,
            max_graph_age_secs: 60,
            changes_since: "1h".into(),
        },
        &proc,
    )
    .is_err());
    let proc = support::scan_postgres_django(&home);
    assert!(twin_app::brief_in(
        &home.layout,
        &BriefRequest {
            target: Some("no-such-thing".into()),
            config_override: None,
            max_graph_age_secs: u64::MAX,
            changes_since: "1h".into(),
        },
        &proc,
    )
    .is_err());
}
