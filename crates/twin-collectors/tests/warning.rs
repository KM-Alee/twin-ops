use twin_collectors::ProcessWarningKind;

#[test]
fn warning_keys_are_stable() {
    assert_eq!(
        ProcessWarningKind::Vanished.aggregate_key(),
        "vanished_processes"
    );
    assert_eq!(ProcessWarningKind::Vanished.detail_key(), "vanished");
    assert!(ProcessWarningKind::Vanished.includes_json_detail());
    assert!(!ProcessWarningKind::ExeUnreadable.includes_json_detail());
    assert_eq!(
        ProcessWarningKind::from_aggregate_key("exe_unreadable"),
        Some(ProcessWarningKind::ExeUnreadable)
    );
}
