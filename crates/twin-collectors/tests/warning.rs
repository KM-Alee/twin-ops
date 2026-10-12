use twin_collectors::ProcessWarningKind;

#[test]
fn warning_keys_are_stable() {
    assert_eq!(
        ProcessWarningKind::Vanished.aggregate_key(),
        "vanished_processes"
    );
    assert_eq!(ProcessWarningKind::Vanished.detail_key(), "vanished");
    assert_eq!(
        ProcessWarningKind::from_aggregate_key("exe_unreadable"),
        Some(ProcessWarningKind::ExeUnreadable)
    );
}

#[test]
fn bulk_coverage_gap_detail_keys_remain_distinguishable() {
    assert_eq!(
        ProcessWarningKind::ExeUnreadable.detail_key(),
        "exe_unreadable"
    );
    assert_eq!(
        ProcessWarningKind::PermissionDenied.detail_key(),
        "permission_denied"
    );
    assert_eq!(
        ProcessWarningKind::CgroupMissing.detail_key(),
        "cgroup_missing"
    );
    assert_eq!(
        ProcessWarningKind::CgroupPermissionDenied.detail_key(),
        "cgroup_permission_denied"
    );
}
