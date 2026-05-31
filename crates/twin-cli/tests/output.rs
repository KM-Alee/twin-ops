use std::path::PathBuf;

use twin_app::{
    DoctorCore, DoctorDatabase, DoctorPermissions, DoctorResult, InitResult, PermissionMode,
};
use twin_cli::output;

#[test]
fn init_render_created() {
    let result = InitResult {
        config_path: PathBuf::from("/tmp/config.toml"),
        db_path: PathBuf::from("/tmp/twin.db"),
        log_path: PathBuf::from("/tmp/twin.log"),
        config_created: true,
        config_updated: false,
        db_created: true,
    };
    let text = output::init::render(&result);
    assert!(text.contains("Twin init: Created"));
}

#[test]
fn init_render_unchanged() {
    let result = InitResult {
        config_path: PathBuf::from("/tmp/config.toml"),
        db_path: PathBuf::from("/tmp/twin.db"),
        log_path: PathBuf::from("/tmp/twin.log"),
        config_created: false,
        config_updated: false,
        db_created: false,
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
fn json_render_parses() {
    let result = InitResult {
        config_path: PathBuf::from("/tmp/config.toml"),
        db_path: PathBuf::from("/tmp/twin.db"),
        log_path: PathBuf::from("/tmp/twin.log"),
        config_created: true,
        config_updated: false,
        db_created: false,
    };
    let json = output::json::render(&result).expect("json");
    serde_json::from_str::<serde_json::Value>(&json).expect("parse");
}
