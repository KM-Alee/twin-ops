mod support;

use std::path::PathBuf;

use tempfile::TempDir;
use twin_collectors::{ProcessCollector, ProcessWarningKind, COLLECTOR_NAME};
use twin_core::TimestampNs;
use twin_observation::{ObservationKind, Pipeline};

use support::{write_proc_fixture, VanishPidReader};

#[test]
fn collects_process_records_from_fixture_proc() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 100 0 0 0 10 5 0 0 20 0 1 0 100 20480 512 4294967295 1 4194304 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t1000\t1000\t1000\t1000\nGid:\t1000\t1000\t1000\t1000\n",
        b"/usr/lib/systemd/systemd\0",
        Some(PathBuf::from("/usr/lib/systemd/systemd").as_path()),
    );
    write_proc_fixture(
        &proc,
        42,
        "42 (nginx) S 1 1 1 0 -1 4194560 50 0 0 0 5 2 0 0 20 0 1 0 200 40960 256 4294967295 1 4194304 0 0 0 0 0 0 0 0 0 0 17 1 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t33\t33\t33\t33\nGid:\t33\t33\t33\t33\n",
        b"/usr/bin/nginx\0-g\0daemon\0off\0",
        Some(PathBuf::from("/usr/bin/nginx").as_path()),
    );

    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.records().len(), 2);
    let nginx = batch
        .records()
        .iter()
        .find(|r| r.pid() == 42)
        .expect("nginx");
    assert_eq!(nginx.ppid(), Some(1));
    assert_eq!(nginx.comm(), Some("nginx"));
    assert_eq!(nginx.uid(), Some(33));
    assert_eq!(nginx.gid(), Some(33));
    assert_eq!(nginx.argv(), &["/usr/bin/nginx", "-g", "daemon", "off"]);
    assert!(nginx.exe().is_some());
}

#[test]
fn handles_comm_with_spaces_in_stat() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        7,
        "7 (my daemon process) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 1 4194304 0 0 0 0 0 0 0 0 0 0 17 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"\0",
        None,
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let record = &batch.records()[0];
    assert_eq!(record.comm(), Some("my daemon process"));
}

#[test]
fn empty_cmdline_is_valid() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        2,
        "2 (kthreadd) S 0 0 0 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"",
        None,
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.records()[0].argv().is_empty());
}

#[test]
fn missing_exe_is_not_vanished_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        2,
        "2 (kthreadd) S 0 0 0 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"",
        None,
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.records().len(), 1);
    assert!(batch.records()[0].exe().is_none());
    assert!(!batch
        .warnings()
        .iter()
        .any(|w| w.kind() == ProcessWarningKind::Vanished));
}

#[test]
fn malformed_stat_becomes_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(&proc, 9, "not valid stat", "", b"", None);
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert!(batch.records().is_empty());
    assert!(batch
        .warnings()
        .iter()
        .any(|w| w.kind() == ProcessWarningKind::Malformed));
}

#[test]
fn vanished_process_becomes_warning() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        1,
        "1 (systemd) S 0 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"systemd\0",
        None,
    );
    let dir_99 = proc.join("99");
    std::fs::create_dir_all(&dir_99).expect("dir");
    let batch = ProcessCollector::with_reader(&proc, VanishPidReader::new(99))
        .collect(TimestampNs::new(1))
        .expect("collect");
    assert_eq!(batch.records().len(), 1);
    assert!(batch
        .warnings()
        .iter()
        .any(|w| w.kind() == ProcessWarningKind::Vanished));
}

#[test]
fn redacts_sensitive_command_values() {
    let tmp = TempDir::new().expect("tempdir");
    let proc = tmp.path().join("proc");
    std::fs::create_dir_all(&proc).expect("proc root");
    write_proc_fixture(
        &proc,
        50,
        "50 (app) S 1 1 1 0 -1 4194560 0 0 0 0 0 0 0 0 20 0 1 0 0 0 0 4294967295 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0 0",
        "Uid:\t0\t0\t0\t0\n",
        b"/usr/bin/app\0--password=sekrit\0--token\0abc\0postgres://user:pass@db:5432/x\0",
        None,
    );
    let batch = ProcessCollector::new(&proc)
        .collect(TimestampNs::new(1))
        .expect("collect");
    let pipeline = Pipeline::default();
    let cmd_obs = batch
        .observations()
        .iter()
        .find(|o| o.kind == ObservationKind::ProcessCommandSeen)
        .expect("command obs");
    let normalized = pipeline.process(cmd_obs.clone()).expect("pipeline");
    let argv = normalized
        .metadata()
        .get("argv_json")
        .and_then(|v| v.as_array())
        .expect("argv");
    let values: Vec<&str> = argv.iter().filter_map(|v| v.as_str()).collect();
    assert!(values.contains(&"/usr/bin/app"));
    assert!(values.iter().any(|v| v.contains("<redacted>")));
    assert_eq!(
        normalized.redaction_state(),
        twin_observation::RedactionState::Partial
    );
}

#[test]
fn collector_name_constant() {
    assert_eq!(COLLECTOR_NAME, "proc_process");
}
