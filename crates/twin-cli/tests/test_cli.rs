mod support;

use support::{stderr_utf8, stdout_utf8, TwinHome};

#[test]
fn test_init_lint_and_run_print_a_report() {
    let home = TwinHome::new();
    assert!(home.run_without_proc(&["init"]).status.success());
    let yaml = home.home_path().join("twin.yaml");
    let path = yaml.to_string_lossy().to_string();
    let init = home.run(&["test", "init", &path]);
    assert!(init.status.success(), "{}", stderr_utf8(&init));
    assert!(stdout_utf8(&init).contains("created"));

    let lint = home.run(&["test", "lint", &path]);
    assert!(lint.status.success(), "{}", stderr_utf8(&lint));
    let lint_text = stdout_utf8(&lint);
    assert!(lint_text.contains("valid"), "{lint_text}");
    assert!(lint_text.contains("local-readiness"), "{lint_text}");

    let run = home.run(&["test", "run", &path]);
    assert!(!run.status.success(), "missing graph nodes should fail");
    let text = stdout_utf8(&run);
    assert!(text.contains("FAIL"), "{text}");
    assert!(text.contains("failed"), "{text}");
    assert!(text.contains("Twin Test: local-readiness"), "{text}");
}

#[test]
fn test_lint_rejects_shell() {
    let home = TwinHome::new();
    let yaml = home.home_path().join("bad.yaml");
    std::fs::write(
        &yaml,
        "version: 1\nname: bad\nchecks:\n  - name: nope\n    shell: true\n    assert:\n      node: service:nginx.service\n      exists: true\n",
    )
    .expect("write");
    let path = yaml.to_string_lossy().to_string();
    let lint = home.run(&["test", "lint", &path]);
    assert!(!lint.status.success());
    assert!(
        stderr_utf8(&lint).contains("shell"),
        "{}",
        stderr_utf8(&lint)
    );
}
