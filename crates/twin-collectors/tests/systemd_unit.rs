use std::collections::HashMap;
use std::path::{Path, PathBuf};

use twin_collectors::UnitFileReader;
use twin_collectors::{SystemdUnitCollector, SystemdWarningKind};

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
fn template_unit_emits_warning() {
    let root = tempfile::tempdir().expect("tempdir");
    let template = root.path().join("postgresql@.service");
    std::fs::write(&template, "[Unit]\nRequires=postgres.service\n").expect("write");
    let collector = SystemdUnitCollector::new(vec![root.path().to_path_buf()]);
    let batch = collector
        .collect(twin_core::TimestampNs::new(1))
        .expect("collect");
    assert!(batch
        .warnings()
        .iter()
        .any(|w| { w.kind() == SystemdWarningKind::GlobInstanceSkipped }));
}

#[test]
fn instance_inherits_template_unit_dependencies() {
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
    assert!(batch.observations().iter().any(|o| {
        o.kind == twin_observation::ObservationKind::SystemdUnitRequires
            && o.metadata.get("from_unit").and_then(|v| v.as_str()) == Some("postgresql@5432.service")
            && o.metadata.get("to_unit").and_then(|v| v.as_str()) == Some("postgres.service")
    }));
}
