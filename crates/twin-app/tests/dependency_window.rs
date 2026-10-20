mod support;

use twin_app::{
    emulate_in, impact_in, scan_in, EmulateActionRequest, EmulateRequest, ImpactRequest,
    ScanRequest,
};
use twin_core::NodeId;

const LISTENER_ONLY: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0";

/// django held a connection to postgres in the first scan; the pool is idle in the second.
fn idle_pool(home: &support::IsolatedHome) -> std::path::PathBuf {
    let proc = support::scan_postgres_django(home);
    support::write_tcp_table(&proc, "tcp", LISTENER_ONLY);
    std::fs::remove_file(proc.join("8841/fd/12")).expect("close client socket");
    scan_in(&home.layout, support::host_isolated_scan(home), &proc).expect("idle scan");
    proc
}

fn impact(
    home: &support::IsolatedHome,
    config: Option<std::path::PathBuf>,
) -> twin_app::ImpactResult {
    impact_in(
        &home.layout,
        ImpactRequest {
            target: Some(NodeId::service("postgresql.service")),
            config_override: config,
            ..ImpactRequest::default()
        },
    )
    .expect("impact")
}

#[test]
fn an_idle_pool_client_stays_a_runtime_dependent_inside_the_window() {
    let home = support::IsolatedHome::new();
    idle_pool(&home);
    let result = impact(&home, None);
    let django = result
        .direct_dependents
        .iter()
        .find(|dependent| dependent.id == "service:django.service")
        .expect("idle client is still a dependent");
    assert_eq!(django.impact_kind, "runtime");
    assert!(
        django
            .reason
            .starts_with("no open connection to postgresql.service now; last seen ")
            && django.reason.ends_with("ago (1×)"),
        "{}",
        django.reason
    );

    let emulation = emulate_in(
        &home.layout,
        EmulateRequest {
            action: EmulateActionRequest::Restart {
                target: Some(NodeId::service("postgresql.service")),
                target_query: None,
                show_paths: false,
                max_depth: 3,
            },
            config_override: None,
            show_evidence: false,
        },
    )
    .expect("emulate");
    assert!(emulation
        .runtime_impacts
        .iter()
        .chain(&emulation.transient_impacts)
        .any(|impact| impact.id == "service:django.service"));
}

#[test]
fn a_connection_older_than_the_window_is_not_a_dependent() {
    let home = support::IsolatedHome::new();
    idle_pool(&home);
    let config = home.layout.config_dir.join("no-window.toml");
    std::fs::write(&config, "[retention]\n[dependencies]\nwindow_days = 0\n").expect("config");
    let result = impact(&home, Some(config));
    assert!(result
        .direct_dependents
        .iter()
        .all(|dependent| dependent.id != "service:django.service"));
}

#[test]
fn an_open_connection_is_listed_once_with_its_live_reason() {
    let home = support::IsolatedHome::new();
    let proc = support::scan_postgres_django(&home);
    scan_in(&home.layout, ScanRequest::default(), &proc).expect("second scan");
    let result = impact(&home, None);
    let django: Vec<_> = result
        .direct_dependents
        .iter()
        .filter(|dependent| dependent.id == "service:django.service")
        .collect();
    assert_eq!(django.len(), 1);
    assert!(
        django[0].reason.starts_with("active connection"),
        "{}",
        django[0].reason
    );
}

const BACKUP_CONNECTED: &str = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   0: 00000000:1538 00000000:0000 0A 00000000:00000000 00000000:00000000  00000000       0        0 12345 1 0000000000000000 100 0 0 10 0\n   1: 0100007F:D431 0100007F:1538 01 00000000:00000000 00000000:00000000  00000000       0        0 777 1 0000000000000000 100 0 0 10 0";

fn scan_with_units(home: &support::IsolatedHome, proc: &std::path::Path, units: &std::path::Path) {
    let _env = support::lock_scan_env();
    support::set_systemd_unit_root(units);
    let result = scan_in(&home.layout, support::host_isolated_scan(home), proc);
    support::clear_scan_env();
    result.expect("scan");
}

#[test]
fn a_timer_run_job_that_has_exited_is_a_dependent_named_with_its_timer() {
    let home = support::IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let units = home.layout.data_dir.join("units");
    std::fs::create_dir_all(&units).expect("units");
    std::fs::write(units.join("backup.timer"), "[Timer]\nOnCalendar=daily\n").expect("timer");
    std::fs::write(units.join("backup.service"), "[Service]\nType=oneshot\n").expect("service");
    std::fs::write(units.join("postgresql.service"), "[Service]\n").expect("postgres");
    let proc = home.layout.data_dir.join("proc");
    std::fs::create_dir_all(&proc).expect("proc");
    for (pid, comm, unit) in [
        (721, "postgres", "postgresql.service"),
        (9100, "pg_dump", "backup.service"),
    ] {
        support::write_proc_fixture_with_cgroup(
            &proc,
            pid,
            &format!("{pid} ({comm}) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0"),
            "Uid:\t0\t0\t0\t0\n",
            format!("/usr/bin/{comm}\0").as_bytes(),
            None,
            Some(&format!("0::/system.slice/{unit}\n")),
        );
    }
    support::write_tcp_table(&proc, "tcp", BACKUP_CONNECTED);
    support::write_socket_fd(&proc, 721, 8, 12345);
    support::write_socket_fd(&proc, 9100, 5, 777);
    scan_with_units(&home, &proc, &units);

    std::fs::remove_dir_all(proc.join("9100")).expect("job exits");
    support::write_tcp_table(&proc, "tcp", LISTENER_ONLY);
    scan_with_units(&home, &proc, &units);

    let store = twin_store::Store::open(&home.layout.db_file()).expect("store");
    let timer = store
        .get_node_typed(&NodeId::service("backup.timer"))
        .expect("read")
        .expect("timer node");
    assert!(
        timer.metadata().as_str().contains(r#""unit_type":"timer""#),
        "{}",
        timer.metadata()
    );
    drop(store);
    let result = impact(&home, None);
    let backup = result
        .direct_dependents
        .iter()
        .find(|dependent| dependent.id == "service:backup.service")
        .expect("the periodic job is still a dependent");
    assert!(
        backup.reason.contains("last seen ")
            && backup
                .reason
                .ends_with("started by backup.timer (schedule unknown)"),
        "{}",
        backup.reason
    );
}
