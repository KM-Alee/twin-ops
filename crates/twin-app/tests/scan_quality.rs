mod support;

use twin_app::assess_scan_quality;
use twin_collectors::{COLLECTOR_NAME, RUNTIME_COLLECTOR_NAME};
use twin_store::{CollectorRunRow, Store};

use support::IsolatedHome;

#[test]
fn assess_scan_quality_good_when_no_warnings() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 0,
            error_message: None,
            metadata_json:
                r#"{"fd_permission_denied":0,"socket_unmapped":0,"active_socket_unmapped":0}"#
                    .to_string(),
        })
        .expect("run");
    let assessment = assess_scan_quality(&store).expect("assess");
    assert_eq!(assessment.quality.as_str(), "good");
    assert!(assessment.impact_reliable);
    assert!(!assessment.ephemeral_capture_recommended);
}

#[test]
fn assess_scan_quality_degraded_when_many_hidden_fds() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 50,
            error_message: None,
            metadata_json:
                r#"{"fd_permission_denied":50,"socket_unmapped":0,"active_socket_unmapped":0}"#
                    .to_string(),
        })
        .expect("run");
    let assessment = assess_scan_quality(&store).expect("assess");
    assert_eq!(assessment.quality.as_str(), "degraded");
    assert!(!assessment.impact_reliable);
}

#[test]
fn assess_scan_quality_notes_dbus_unavailable() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: RUNTIME_COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 1,
            error_message: None,
            metadata_json: r#"{"dbus_available":false,"enable_symlink_count":0}"#.to_string(),
        })
        .expect("run");
    let assessment = assess_scan_quality(&store).expect("assess");
    assert!(assessment
        .reasons
        .iter()
        .any(|r| r.contains("D-Bus unavailable")));
}

#[test]
fn assess_scan_quality_recommends_ephemeral_capture_for_single_sample_connections() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 0,
            error_message: None,
            metadata_json: r#"{"fd_permission_denied":0,"socket_unmapped":0,"active_socket_unmapped":0,"samples_total":1,"tcp_connection_count":2}"#
                .to_string(),
        })
        .expect("run");
    let assessment = assess_scan_quality(&store).expect("assess");
    assert!(assessment.ephemeral_capture_recommended);
}

#[test]
fn assess_scan_quality_skips_ephemeral_capture_after_multi_sample() {
    let home = IsolatedHome::new();
    twin_app::init_in(&home.layout, twin_app::InitRequest::default()).expect("init");
    let mut store = Store::open(&home.layout.db_file()).expect("open");
    store
        .insert_collector_run(&CollectorRunRow {
            id: None,
            collector: COLLECTOR_NAME.to_string(),
            started_at_ns: 1,
            ended_at_ns: 2,
            status: "success".to_string(),
            observation_count: 0,
            warning_count: 0,
            error_message: None,
            metadata_json: r#"{"fd_permission_denied":0,"socket_unmapped":0,"active_socket_unmapped":0,"samples_total":3,"tcp_connection_count":2}"#
                .to_string(),
        })
        .expect("run");
    let assessment = assess_scan_quality(&store).expect("assess");
    assert!(!assessment.ephemeral_capture_recommended);
}
