use std::path::Path;

use twin_test::{parse_str, write_starter, CheckKind, STARTER_YAML};

#[test]
fn starter_parses_and_names_every_check_kind() {
    let doc = parse_str(STARTER_YAML, Path::new("twin.yaml")).expect("starter");
    assert_eq!(doc.name, "local-readiness");
    assert_eq!(doc.checks.len(), 8);
    assert!(matches!(doc.checks[0].kind, CheckKind::Node(_)));
    assert!(matches!(doc.checks[1].kind, CheckKind::Port(_)));
    assert!(matches!(doc.checks[2].kind, CheckKind::Service(_)));
    assert!(matches!(doc.checks[3].kind, CheckKind::Dependency { .. }));
    assert!(matches!(doc.checks[4].kind, CheckKind::Emulate { .. }));
    assert!(matches!(doc.checks[5].kind, CheckKind::Outbound { .. }));
    assert!(matches!(doc.checks[6].kind, CheckKind::Disk { .. }));
    assert!(matches!(doc.checks[7].kind, CheckKind::Unknowns { .. }));
}

#[test]
fn unknown_version_duplicate_name_and_bad_port_are_rejected() {
    let err = parse_str("version: 2\nname: x\nchecks: []\n", Path::new("twin.yaml"))
        .expect_err("version");
    assert!(err.to_string().contains("version"), "{err}");

    let dup = r#"
version: 1
name: suite
checks:
  - name: same
    assert:
      node: service:nginx.service
      exists: true
  - name: same
    assert:
      node: service:nginx.service
      exists: true
"#;
    let err = parse_str(dup, Path::new("twin.yaml")).expect_err("dup");
    assert!(err.to_string().contains("duplicate"), "{err}");

    let port = r#"
version: 1
name: suite
checks:
  - name: not a port
    assert:
      port: service:nginx.service
      exists: true
"#;
    let err = parse_str(port, Path::new("twin.yaml")).expect_err("port");
    assert!(err.to_string().contains("port"), "{err}");
}

#[test]
fn shell_execution_is_rejected() {
    let yaml = r#"
version: 1
name: suite
checks:
  - name: do not run
    shell: systemctl restart nginx
    assert:
      node: service:nginx.service
      exists: true
"#;
    let err = parse_str(yaml, Path::new("twin.yaml")).expect_err("shell");
    assert!(err.to_string().contains("shell"), "{err}");
}

#[test]
fn write_starter_refuses_to_overwrite_without_force() {
    let dir = tempfile::tempdir().expect("temp");
    let path = dir.path().join("twin.yaml");
    write_starter(&path, false).expect("write");
    let err = write_starter(&path, false).expect_err("exists");
    assert!(err.to_string().contains("already exists"), "{err}");
    write_starter(&path, true).expect("force");
}
