use twin_app::{CheckResult, CheckStatus, TestRunReport};
use twin_cli::output;

#[test]
fn test_run_output_has_pass_warn_fail_and_json() {
    let report = TestRunReport {
        name: "local-readiness".to_string(),
        checks: vec![
            CheckResult {
                name: "postgres is listening".to_string(),
                status: CheckStatus::Pass,
                detail: None,
            },
            CheckResult {
                name: "stale listener".to_string(),
                status: CheckStatus::Warn,
                detail: Some("port:tcp:127.0.0.1:5432 is stale".to_string()),
            },
            CheckResult {
                name: "django depends on postgres".to_string(),
                status: CheckStatus::Fail,
                detail: Some(
                    "service:django.service depends_on service:postgresql.service evidence is weak, minimum is moderate"
                        .to_string(),
                ),
            },
        ],
        passed: 1,
        warned: 1,
        failed: 1,
    };
    let text = output::test_report::render_run(&report);
    assert!(text.contains("twin test"));
    assert!(text.contains("Twin Test: local-readiness"));
    assert!(text.contains("PASS"));
    assert!(text.contains("WARN"));
    assert!(text.contains("FAIL"));
    assert!(text.contains("passed"));
    assert!(text.contains("warned"));
    assert!(text.contains("failed"));
    let json = output::json::render(&report).expect("json");
    assert!(json.contains("\"failed\": 1"));
    assert!(json.contains("minimum is moderate"));
}
