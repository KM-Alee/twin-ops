use std::path::PathBuf;

use tempfile::TempDir;
use twin_collectors::SystemdRuntimeCollector;
use twin_core::TimestampNs;
use twin_observation::ObservationKind;

#[test]
fn collects_wants_symlink() {
    let tmp = TempDir::new().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let wants = root.join("multi-user.target.wants");
    std::fs::create_dir_all(&wants).expect("wants dir");
    std::os::unix::fs::symlink("../docker.service", wants.join("docker.service")).expect("symlink");
    std::fs::write(root.join("docker.service"), "[Unit]\nDescription=Docker\n").expect("unit");

    let batch = SystemdRuntimeCollector::new(vec![root])
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.enable_symlink_count() >= 1);
    assert!(batch.observations().iter().any(|o| {
        o.kind == ObservationKind::SystemdUnitWantedBy
            && o.metadata.get("from_unit").and_then(|v| v.as_str()) == Some("multi-user.target")
            && o.metadata.get("to_unit").and_then(|v| v.as_str()) == Some("docker.service")
    }));
}
