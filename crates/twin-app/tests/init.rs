mod support;

use std::fs;

use twin_app::{doctor_in, init_in, read_config, InitRequest};
use twin_core::config::{TwinConfig, DEFAULT_CONFIG_TOML};
use twin_store::LATEST_VERSION;

#[test]
fn creates_config_and_database() {
    let home = support::IsolatedHome::new();
    let result = init_in(&home.layout, InitRequest::default()).expect("init");

    assert!(result.config_path.exists());
    assert!(result.db_path.exists());
    assert!(result.config_created);
    assert!(result.db_created);
    assert_eq!(result.schema_version, LATEST_VERSION);
}

#[test]
fn init_and_doctor_agree_on_schema_version() {
    let home = support::IsolatedHome::new();
    let init = init_in(&home.layout, InitRequest::default()).expect("init");
    let doctor = doctor_in(&home.layout, None).expect("doctor");
    assert_eq!(init.schema_version, LATEST_VERSION);
    assert_eq!(doctor.database.schema_version, Some(init.schema_version));
}

#[test]
fn second_run_is_idempotent() {
    let home = support::IsolatedHome::new();
    let first = init_in(&home.layout, InitRequest::default()).expect("first");
    let second = init_in(&home.layout, InitRequest::default()).expect("second");

    assert!(first.config_created);
    assert!(first.db_created);
    assert!(!second.config_created);
    assert!(!second.config_updated);
    assert!(!second.db_created);
}

#[test]
fn force_rewrites_config() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("first");
    let second = init_in(
        &home.layout,
        InitRequest {
            force: true,
            ..InitRequest::default()
        },
    )
    .expect("force");

    assert!(second.config_updated);
}

#[test]
fn refreshes_stale_default_template() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("first");

    let cfg_path = home.layout.config_file(None);
    let stale = "# twin configuration\n# old comment\n\n[retention]\nraw_observations_days = 7\ngraph_history_days = 30\nebpf_events_days = 3\n";
    fs::write(&cfg_path, stale).expect("write stale");

    let second = init_in(&home.layout, InitRequest::default()).expect("refresh");
    assert!(!second.config_created);
    assert!(second.config_updated);
    assert_eq!(
        fs::read_to_string(&cfg_path).expect("read"),
        DEFAULT_CONFIG_TOML
    );
}

#[test]
fn leaves_customized_config_untouched() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("first");

    let cfg_path = home.layout.config_file(None);
    let custom =
        "[retention]\nraw_observations_days = 14\ngraph_history_days = 30\nebpf_events_days = 3\n";
    fs::write(&cfg_path, custom).expect("write custom");

    let second = init_in(&home.layout, InitRequest::default()).expect("second");
    assert!(!second.config_created);
    assert!(!second.config_updated);
    assert!(fs::read_to_string(&cfg_path)
        .expect("read")
        .contains("raw_observations_days = 14"));
}

#[test]
fn config_roundtrip_matches_defaults() {
    let home = support::IsolatedHome::new();
    init_in(&home.layout, InitRequest::default()).expect("init");

    let cfg = read_config(&home.layout.config_file(None)).expect("read");
    assert_eq!(cfg, TwinConfig::default());
}
