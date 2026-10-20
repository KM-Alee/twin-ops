use std::collections::HashMap;
use std::path::{Path, PathBuf};

use twin_collectors::SystemdUnitCollector;
use twin_collectors::UnitFileReader;

struct MapUnitReader(HashMap<PathBuf, String>);

impl UnitFileReader for MapUnitReader {
    fn read_to_string(&self, path: &Path) -> Result<String, String> {
        self.0
            .get(path)
            .cloned()
            .ok_or_else(|| "missing fixture".to_string())
    }
}

fn fixture_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/systemd")
}

#[test]
fn collects_requires_from_docker_unit() {
    let root = fixture_root();
    let docker = root.join("docker.service");
    let containerd = root.join("containerd.service");
    let mut files = HashMap::new();
    files.insert(
        docker.clone(),
        std::fs::read_to_string(&docker).expect("docker.service"),
    );
    files.insert(
        containerd.clone(),
        std::fs::read_to_string(&containerd).expect("containerd.service"),
    );
    let collector = SystemdUnitCollector::with_reader(vec![root], MapUnitReader(files));
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    assert!(batch.units_scanned() >= 2);
    assert!(batch.dependency_count() >= 1);
    assert!(batch
        .observations()
        .iter()
        .any(|o| o.kind == twin_observation::ObservationKind::SystemdUnitRequires));
}

#[test]
fn dropin_appends_requires() {
    let root = fixture_root();
    let docker = root.join("docker.service");
    let dropin = root.join("docker.service.d/override.conf");
    let mut files = HashMap::new();
    files.insert(
        docker.clone(),
        std::fs::read_to_string(&docker).expect("docker.service"),
    );
    files.insert(
        dropin.clone(),
        std::fs::read_to_string(&dropin).expect("override"),
    );
    let collector = SystemdUnitCollector::with_reader(vec![root], MapUnitReader(files));
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    let requires: Vec<_> = batch
        .observations()
        .iter()
        .filter(|o| o.kind == twin_observation::ObservationKind::SystemdUnitRequires)
        .collect();
    assert!(requires.len() >= 2);
}

#[test]
fn missing_systemd_dir_returns_empty_batch() {
    let collector = SystemdUnitCollector::new(vec![PathBuf::from("/nonexistent/systemd")]);
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.units_scanned(), 0);
    assert!(batch.observations().is_empty());
}

#[test]
fn dropin_under_etc_merges_when_main_unit_in_usr_lib() {
    let etc = tempfile::tempdir().expect("etc");
    let usr = tempfile::tempdir().expect("usr");
    let docker_usr = usr.path().join("docker.service");
    std::fs::write(&docker_usr, "[Unit]\nRequires=containerd.service\n").expect("write");
    let dropin_dir = etc.path().join("docker.service.d");
    std::fs::create_dir_all(&dropin_dir).expect("dir");
    std::fs::write(
        dropin_dir.join("override.conf"),
        "[Unit]\nRequires=network-online.target\n",
    )
    .expect("dropin");
    let collector =
        SystemdUnitCollector::new(vec![etc.path().to_path_buf(), usr.path().to_path_buf()]);
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    let requires: Vec<_> = batch
        .observations()
        .iter()
        .filter(|o| o.kind == twin_observation::ObservationKind::SystemdUnitRequires)
        .filter_map(|o| {
            o.metadata
                .get("to_unit")
                .and_then(|v| v.as_str().map(str::to_string))
        })
        .collect();
    assert!(requires.iter().any(|u| u == "containerd.service"));
    assert!(requires.iter().any(|u| u == "network-online.target"));
}

#[test]
fn uninstantiated_template_is_not_a_missing_service() {
    let root = tempfile::tempdir().expect("tempdir");
    let template = root.path().join("postgresql@.service");
    std::fs::write(&template, "[Unit]\nRequires=postgres.service\n").expect("write");
    let collector = SystemdUnitCollector::new(vec![root.path().to_path_buf()]);
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.units_scanned(), 0);
    assert!(batch.warnings().is_empty());
}

#[test]
fn explicit_instance_file_takes_precedence_over_template() {
    let root = tempfile::tempdir().expect("tempdir");
    std::fs::write(
        root.path().join("postgresql@.service"),
        "[Unit]\nRequires=postgres.service\n",
    )
    .expect("template");
    std::fs::write(
        root.path().join("postgresql@5432.service"),
        "[Unit]\nDescription=postgres instance\n",
    )
    .expect("instance");
    let collector = SystemdUnitCollector::new(vec![root.path().to_path_buf()]);
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    assert!(!batch.observations().iter().any(|o| {
        o.kind == twin_observation::ObservationKind::SystemdUnitRequires
            && o.metadata.get("from_unit").and_then(|v| v.as_str())
                == Some("postgresql@5432.service")
            && o.metadata.get("to_unit").and_then(|v| v.as_str()) == Some("postgres.service")
    }));
}

#[test]
fn enabled_and_runtime_instances_expand_template_and_dropin_dependencies() {
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().expect("root");
    std::fs::write(
        root.path().join("worker@.service"),
        "[Unit]\nRequires=database@%i.service\nWants=%p-helper.service\n",
    )
    .expect("template");
    let enabled = root.path().join("multi-user.target.wants");
    std::fs::create_dir(&enabled).expect("enabled");
    symlink("../worker@.service", enabled.join("worker@blue.service")).expect("link");
    let dropin = root.path().join("worker@.service.d");
    std::fs::create_dir(&dropin).expect("dropin");
    std::fs::write(
        dropin.join("10-extra.conf"),
        "[Unit]\nRequires=cache@%i.service\n",
    )
    .expect("extra");
    let instance_dropin = root.path().join("worker@blue.service.d");
    std::fs::create_dir(&instance_dropin).expect("instance dropin");
    std::fs::write(
        instance_dropin.join("10-extra.conf"),
        "[Unit]\nRequires=blue-only.service\n",
    )
    .expect("specific");
    let batch = SystemdUnitCollector::new(vec![root.path().to_path_buf()])
        .collect_with_instances(
            twin_core::TimestampNs::new(1),
            &["worker@green.service".into()],
        )
        .expect("collect");
    assert_eq!(batch.units_scanned(), 2);
    let blue = batch
        .units()
        .iter()
        .find(|unit| unit.unit_name == "worker@blue.service")
        .expect("blue");
    assert!(blue
        .dependencies
        .requires
        .contains(&"database@blue.service".into()));
    assert!(blue
        .dependencies
        .requires
        .contains(&"blue-only.service".into()));
    assert!(!blue
        .dependencies
        .requires
        .contains(&"cache@blue.service".into()));
    assert!(blue
        .dependencies
        .wants
        .contains(&"worker-helper.service".into()));
    let green = batch
        .units()
        .iter()
        .find(|unit| unit.unit_name == "worker@green.service")
        .expect("green");
    assert!(green
        .dependencies
        .requires
        .contains(&"cache@green.service".into()));
    assert!(green
        .dependencies
        .requires
        .contains(&"database@green.service".into()));
    assert!(batch.warnings().is_empty());
}

#[test]
fn higher_priority_main_files_masks_and_same_named_dropins_win() {
    use std::os::unix::fs::symlink;
    let etc = tempfile::tempdir().expect("etc");
    let usr = tempfile::tempdir().expect("usr");
    for (root, target) in [
        (etc.path(), "correct.service"),
        (usr.path(), "wrong.service"),
    ] {
        std::fs::write(
            root.join("api.service"),
            format!("[Unit]\nRequires={target}\n"),
        )
        .expect("main");
        std::fs::create_dir(root.join("api.service.d")).expect("dropin");
        std::fs::write(
            root.join("api.service.d/10-extra.conf"),
            format!("[Unit]\nWants={target}\n"),
        )
        .expect("extra");
    }
    std::fs::write(
        usr.path().join("masked.service"),
        "[Unit]\nRequires=wrong.service\n",
    )
    .expect("masked main");
    symlink("/dev/null", etc.path().join("masked.service")).expect("mask");
    let batch = SystemdUnitCollector::new(vec![etc.path().to_path_buf(), usr.path().to_path_buf()])
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    let api = batch
        .units()
        .iter()
        .find(|unit| unit.unit_name == "api.service")
        .expect("api");
    assert_eq!(api.dependencies.requires, ["correct.service"]);
    assert_eq!(api.dependencies.wants, ["correct.service"]);
    assert!(!batch
        .units()
        .iter()
        .any(|unit| unit.unit_name == "masked.service"));
    assert!(batch
        .warnings()
        .iter()
        .any(|warning| warning.kind() == twin_collectors::SystemdWarningKind::MaskedEmpty));
}

#[test]
fn socket_declarations_handle_accept_templates_datagrams_fifos_and_vsock() {
    let root = tempfile::tempdir().expect("root");
    for (unit, config) in [
        ("api.socket", "ListenStream=vsock:3:8080\nAccept=yes\n"),
        ("dns.socket", "ListenDatagram=53\nAccept=yes\n"),
        ("pipe.socket", "ListenFIFO=/run/api.fifo\nAccept=yes\n"),
        (
            "invalid.socket",
            "ListenStream=80\nAccept=yes\nService=regular.service\n",
        ),
        ("bad-bool.socket", "ListenStream=80\nAccept=maybe\n"),
    ] {
        std::fs::write(root.path().join(unit), format!("[Socket]\n{config}")).expect("socket");
    }
    let batch = SystemdUnitCollector::new(vec![root.path().to_path_buf()])
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    let targets: Vec<_> = batch
        .observations()
        .iter()
        .filter(|obs| obs.kind == twin_observation::ObservationKind::SystemdSocketActivates)
        .filter_map(|obs| {
            obs.metadata
                .get("service_unit")
                .and_then(|value| value.as_str())
        })
        .collect();
    assert!(targets.contains(&"api@.service"));
    assert!(targets.contains(&"dns.service"));
    assert!(targets.contains(&"pipe.service"));
    assert!(!targets.contains(&"regular.service"));
    let pipe = batch
        .observations()
        .iter()
        .find(|obs| {
            obs.kind == twin_observation::ObservationKind::SystemdSocketSeen
                && obs.metadata.get("unit").and_then(|value| value.as_str()) == Some("pipe.socket")
        })
        .expect("pipe");
    assert_eq!(
        pipe.metadata.get("listen_fifos").expect("FIFO")[0],
        "/run/api.fifo"
    );
    assert!(batch.warnings().len() >= 2);
}

#[test]
fn socket_dropins_reset_all_addresses_and_override_service_and_accept() {
    use twin_observation::ObservationKind;
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("api.socket"), "[Unit]\nRequires=network.target\n[Socket]\nListenStream=8000\nListenDatagram=8001\nListenFIFO=/run/old\nAccept=yes\nService=old@.service\n").unwrap();
    let dropins = root.path().join("api.socket.d");
    std::fs::create_dir(&dropins).unwrap();
    std::fs::write(
        dropins.join("10-reset.conf"),
        "[Socket]\nListenStream=\nListenStream=9000\nAccept=no\nService=new.service\n",
    )
    .unwrap();
    std::fs::write(
        dropins.join("20-add.conf"),
        "[Socket]\nListenDatagram=9001\n",
    )
    .unwrap();
    std::fs::write(
        dropins.join("30-invalid.conf"),
        "[Unit]\nRequires=must-not-apply.service\n[Socket]\nListenStream=9999\nAccept=invalid\n",
    )
    .unwrap();
    let batch = SystemdUnitCollector::new(vec![root.path().to_path_buf()])
        .collect(twin_core::TimestampNs::new(1))
        .unwrap();
    let socket = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::SystemdSocketSeen)
        .unwrap();
    let meta = socket.metadata.to_json();
    assert!(meta.contains("9000"), "{meta}");
    assert!(meta.contains("9001"), "{meta}");
    for excluded in ["8000", "8001", "/run/old", "9999"] {
        assert!(!meta.contains(excluded), "{meta}");
    }
    assert!(batch
        .observations()
        .iter()
        .any(|o| o.kind == ObservationKind::SystemdSocketActivates
            && o.metadata.to_json().contains("new.service")));
    assert!(batch
        .observations()
        .iter()
        .any(|o| o.kind == ObservationKind::SystemdUnitRequires));
    assert_eq!(batch.warnings().len(), 1);
    assert!(!batch
        .observations()
        .iter()
        .any(|obs| obs.metadata.to_json().contains("must-not-apply.service")));
}

#[test]
fn unit_syntax_supports_continuations_comments_and_quoted_dependencies() {
    let parsed = twin_collectors::systemd::parse_unit_file(std::path::Path::new("api.service"), "[Unit]\nDescription=User's application\nRequires=\"db.service\" \\\n# ignored continuation comment\n; another ignored comment\n 'cache.service'\nWants=metrics.service\n").expect("parse");
    assert_eq!(parsed.requires, ["db.service", "cache.service"]);
    assert_eq!(parsed.wants, ["metrics.service"]);
    for value in [
        "Requires='unterminated",
        "Requires=\"api.service\"suffix.service",
        "Requires=bad\\q.service",
        "Requires=db.service \\",
    ] {
        assert!(twin_collectors::systemd::parse_unit_file(
            std::path::Path::new("api.service"),
            &format!("[Unit]\n{value}")
        )
        .is_err());
    }
}

#[test]
fn prefix_and_type_dropins_obey_specificity_and_masks() {
    let root = tempfile::tempdir().expect("fixture");
    let high = root.path().join("etc");
    let low = root.path().join("vendor");
    for dir in [
        high.join("service.d"),
        high.join("foo-.service.d"),
        low.join("foo-bar.service.d"),
    ] {
        std::fs::create_dir_all(dir).unwrap();
    }
    std::fs::write(low.join("foo-bar.service"), "[Unit]\nDescription=api\n").unwrap();
    std::fs::write(
        high.join("service.d/10-same.conf"),
        "[Unit]\nRequires=generic.service\n",
    )
    .unwrap();
    std::fs::write(
        high.join("foo-.service.d/10-same.conf"),
        "[Unit]\nRequires=prefix.service\n",
    )
    .unwrap();
    std::fs::write(
        low.join("foo-bar.service.d/10-same.conf"),
        "[Unit]\nRequires=specific.service\n",
    )
    .unwrap();
    std::fs::write(
        high.join("service.d/20-other.conf"),
        "[Unit]\nWants=metrics.service\n",
    )
    .unwrap();
    let collector = SystemdUnitCollector::new(vec![high.clone(), low.clone()]);
    let batch = collector.collect(twin_core::TimestampNs::new(1)).unwrap();
    let unit = batch
        .units()
        .iter()
        .find(|unit| unit.unit_name == "foo-bar.service")
        .unwrap();
    assert_eq!(unit.dependencies.requires, ["specific.service"]);
    assert_eq!(unit.dependencies.wants, ["metrics.service"]);
    #[cfg(unix)]
    {
        std::fs::remove_file(low.join("foo-bar.service.d/10-same.conf")).unwrap();
        std::os::unix::fs::symlink("/dev/null", low.join("foo-bar.service.d/10-same.conf"))
            .unwrap();
        let batch = collector.collect(twin_core::TimestampNs::new(2)).unwrap();
        let unit = batch
            .units()
            .iter()
            .find(|unit| unit.unit_name == "foo-bar.service")
            .unwrap();
        assert!(unit.dependencies.requires.is_empty());
    }
}

#[test]
fn unescaped_instance_and_filename_specifiers_expand_without_host_context() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("worker@.socket"),
        "[Socket]\nListenStream=/run/%I.sock\nListenFIFO=%f\nService=worker@%i.service\n",
    )
    .unwrap();
    let collector = SystemdUnitCollector::new(vec![root.path().to_path_buf()]);
    let batch = collector
        .collect_with_instances(
            twin_core::TimestampNs::new(1),
            &["worker@var-lib.socket".to_string()],
        )
        .unwrap();
    let unit = batch
        .units()
        .iter()
        .find(|unit| unit.unit_name == "worker@var-lib.socket")
        .unwrap();
    let socket = unit.socket_config.as_ref().unwrap();
    assert_eq!(socket.listen_streams, ["/run/var/lib.sock"]);
    assert_eq!(socket.listen_fifos, ["/var/lib"]);
    assert_eq!(socket.service.as_deref(), Some("worker@var-lib.service"));
}

#[test]
fn oversized_unit_and_malformed_socket_dependencies_are_explicit_gaps() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("large.service"),
        vec![b' '; 1024 * 1024 + 1],
    )
    .unwrap();
    std::fs::write(
        root.path().join("bad.socket"),
        "[Unit]\nRequires='unterminated\n[Socket]\nListenStream=9000\n",
    )
    .unwrap();
    let collector = SystemdUnitCollector::new(vec![root.path().to_path_buf()]);
    let batch = collector.collect(twin_core::TimestampNs::new(1)).unwrap();
    assert_eq!(batch.units_scanned(), 0);
    assert!(batch
        .warnings()
        .iter()
        .any(|warning| warning.detail().contains("read limit")));
    assert!(batch
        .warnings()
        .iter()
        .any(|warning| warning.detail().contains("unterminated")));
    assert!(batch.observations().is_empty());
}

#[test]
fn timers_declare_the_unit_they_start_with_drop_in_overrides() {
    let root = tempfile::tempdir().expect("root");
    for (unit, content) in [
        ("backup.timer", "[Timer]\nOnCalendar=daily\n"),
        (
            "rotate.timer",
            "[Timer]\nOnCalendar=hourly\nUnit=logrotate.service\n",
        ),
        (
            "moved.timer",
            "[Timer]\nOnCalendar=weekly\nUnit=old.service\n",
        ),
        ("templated.timer", "[Timer]\nUnit=job@%i.service\n"),
        ("broken.timer", "[Unit]\nDescription=no timer section\n"),
    ] {
        std::fs::write(root.path().join(unit), content).expect("timer");
    }
    std::fs::create_dir_all(root.path().join("moved.timer.d")).expect("drop-in dir");
    std::fs::write(
        root.path().join("moved.timer.d/override.conf"),
        "[Timer]\nUnit=new.service\n",
    )
    .expect("drop-in");
    let batch = SystemdUnitCollector::new(vec![root.path().to_path_buf()])
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    let mut starts: Vec<_> = batch
        .observations()
        .iter()
        .filter(|obs| obs.kind == twin_observation::ObservationKind::SystemdTimerActivates)
        .map(|obs| {
            let field = |key: &str| {
                obs.metadata
                    .get(key)
                    .and_then(|value| value.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            (field("timer_unit"), field("service_unit"))
        })
        .collect();
    starts.sort();
    assert_eq!(
        starts,
        [
            ("backup.timer".to_string(), "backup.service".to_string()),
            ("moved.timer".to_string(), "new.service".to_string()),
            ("rotate.timer".to_string(), "logrotate.service".to_string()),
        ]
    );
    let warned: Vec<_> = batch
        .warnings()
        .iter()
        .filter_map(|warning| warning.path().file_name())
        .map(|name| name.to_string_lossy().to_string())
        .collect();
    assert!(warned.contains(&"broken.timer".to_string()), "{warned:?}");
    assert!(
        warned.contains(&"templated.timer".to_string()),
        "{warned:?}"
    );
}
