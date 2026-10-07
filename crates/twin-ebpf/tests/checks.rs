use std::fs;

use twin_ebpf::{assess, read_facts, EbpfFacts, EbpfProbePaths};

fn ready_facts() -> EbpfFacts {
    EbpfFacts {
        kernel_release: "6.8.0".to_string(),
        btf_present: true,
        effective_caps: Some(1u64 << 21),
        exec_tracepoint_present: true,
    }
}

#[test]
fn all_checks_pass_for_a_ready_host() {
    let report = assess(&ready_facts());
    assert!(report.is_ready());
    assert_eq!(report.kernel.status, "supported");
    assert_eq!(report.btf.status, "available");
    assert_eq!(report.capabilities.status, "available");
    assert_eq!(report.exec_tracing.status, "available");
}

#[test]
fn each_check_fails_alone() {
    let mut facts = ready_facts();
    facts.kernel_release = "5.4.0-1-generic".to_string();
    let report = assess(&facts);
    assert!(!report.kernel.ok);
    assert!(report.btf.ok && report.capabilities.ok && report.exec_tracing.ok);
    assert!(report.failure_summary().contains("kernel"));
    assert_eq!(report.kernel.status, "unsupported");

    let mut facts = ready_facts();
    facts.btf_present = false;
    let report = assess(&facts);
    assert!(!report.btf.ok);
    assert!(report.kernel.ok && report.capabilities.ok && report.exec_tracing.ok);
    assert!(report.failure_summary().contains("BTF"));

    let mut facts = ready_facts();
    facts.effective_caps = Some(1u64 << 40);
    let report = assess(&facts);
    assert!(!report.capabilities.ok);
    assert!(report.kernel.ok && report.btf.ok && report.exec_tracing.ok);
    assert!(report.failure_summary().contains("capabilities"));

    let mut facts = ready_facts();
    facts.effective_caps = None;
    let report = assess(&facts);
    assert!(!report.capabilities.ok);
    assert_eq!(report.capabilities.status, "unavailable");

    let mut facts = ready_facts();
    facts.exec_tracepoint_present = false;
    let report = assess(&facts);
    assert!(!report.exec_tracing.ok);
    assert!(report.kernel.ok && report.btf.ok && report.capabilities.ok);
    assert!(report.failure_summary().contains("exec tracing"));
}

#[test]
fn bpf_and_perfmon_together_are_enough() {
    let mut facts = ready_facts();
    facts.effective_caps = Some((1u64 << 40) | (1u64 << 38));
    assert!(assess(&facts).capabilities.ok);
}

#[test]
fn unreadable_kernel_release_is_unsupported() {
    let mut facts = ready_facts();
    facts.kernel_release = "not-a-version".to_string();
    let report = assess(&facts);
    assert!(!report.kernel.ok);
    assert_eq!(report.kernel.status, "unsupported");

    facts.kernel_release = "5".to_string();
    assert!(!assess(&facts).kernel.ok);

    facts.kernel_release.clear();
    assert!(!assess(&facts).kernel.ok);
}

#[test]
fn ring_buffer_needs_kernel_5_8() {
    let mut facts = ready_facts();
    facts.kernel_release = "5.8.0".to_string();
    assert!(assess(&facts).kernel.ok);
    facts.kernel_release = "5.7.9".to_string();
    assert!(!assess(&facts).kernel.ok);
}

#[test]
fn missing_files_are_unsupported_not_a_crash() {
    let root = tempfile::tempdir().expect("tempdir");
    let paths = EbpfProbePaths {
        kernel_release: root.path().join("osrelease"),
        btf: root.path().join("vmlinux"),
        status: root.path().join("status"),
        exec_tracepoints: vec![root.path().join("sched_process_exec")],
    };
    let facts = read_facts(&paths);
    let report = assess(&facts);
    assert!(!report.is_ready());
    assert!(!report.kernel.ok);
    assert!(!report.btf.ok);
    assert!(!report.capabilities.ok);
    assert!(!report.exec_tracing.ok);

    fs::write(root.path().join("osrelease"), "nope\n").expect("write");
    fs::write(root.path().join("vmlinux"), "").expect("write");
    fs::write(root.path().join("status"), "Name:\tfoo\nCapEff:\tzz\n").expect("write");
    fs::create_dir(root.path().join("sched_process_exec")).expect("dir");
    let facts = read_facts(&paths);
    let report = assess(&facts);
    assert!(!report.kernel.ok);
    assert!(!report.btf.ok);
    assert!(!report.capabilities.ok);
    assert!(report.exec_tracing.ok);

    fs::write(root.path().join("osrelease"), "6.1.0-1\n").expect("write");
    fs::write(root.path().join("vmlinux"), [0u8, 1, 2, 3]).expect("write");
    fs::write(root.path().join("status"), "CapEff:\t0000000000200000\n").expect("write");
    let report = assess(&read_facts(&paths));
    assert!(report.kernel.ok);
    assert!(report.btf.ok);
    assert!(report.capabilities.ok);
}
