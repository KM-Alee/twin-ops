mod support;

use tempfile::TempDir;
use twin_collectors::{ProcessCollector, ProcessWarningKind, COLLECTOR_NAME};
use twin_core::TimestampNs;
use twin_observation::{ObservationKind, ObservationSource, Pipeline};

use support::write_proc_fixture_with_cgroup;

#[test]
fn v2_systemd_service_emits_cgroup_observation() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture_with_cgroup(
        &proc,
        1432,
        "1432 (nginx) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\n",
        b"/usr/sbin/nginx\0",
        None,
        Some("0::/system.slice/nginx.service\n"),
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let record = &batch.records()[0];
    assert_eq!(record.cgroup_memberships().len(), 1);
    assert_eq!(
        record.cgroup_memberships()[0].service_unit.as_deref(),
        Some("nginx.service")
    );
    let obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::ProcessBelongsToCgroup)
        .expect("cgroup obs");
    assert_eq!(obs.source, ObservationSource::ProcCgroup);
    let normalized = Pipeline::default().process(obs.clone()).expect("pipeline");
    assert_eq!(
        normalized.subject().map(|s| s.as_str()),
        Some("process:pid:1432")
    );
    assert_eq!(
        normalized.object().map(|o| o.as_str()),
        Some("cgroup:/system.slice/nginx.service")
    );
}

#[test]
fn non_systemd_cgroup_has_no_service_inference() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture_with_cgroup(
        &proc,
        99,
        "99 (bash) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\n",
        b"bash\0",
        None,
        Some("0::/user.slice/user-1000.slice/session-2.scope\n"),
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.records()[0].cgroup_memberships()[0]
        .service_unit
        .is_none());
    assert!(!batch.observations().iter().any(|o| {
        o.metadata
            .get("service_inference")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    }));
}

#[test]
fn malformed_cgroup_line_warns_and_keeps_valid_lines() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture_with_cgroup(
        &proc,
        10,
        "10 (app) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"app\0",
        None,
        Some("bad-line\n0::/system.slice/app.service\n"),
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.records()[0].cgroup_memberships().len(), 1);
    assert!(batch
        .warnings()
        .iter()
        .any(|w| w.kind() == ProcessWarningKind::CgroupMalformed));
}

#[test]
fn collector_name_constant() {
    assert_eq!(COLLECTOR_NAME, "proc_process");
}
