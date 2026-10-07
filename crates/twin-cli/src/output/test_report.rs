use std::path::Path;

use twin_app::{CheckStatus, TestDocument, TestInitOutcome, TestRunReport};

use crate::output::format::{Lines, Status};

pub fn render_init(result: &TestInitOutcome) -> String {
    let mut out = Lines::new();
    out.title("twin test init");
    out.status_row(Status::Ok, "file", "created");
    out.blank();
    out.section("path");
    out.path_row(true, "twin.yaml", Path::new(&result.path), None);
    out.into_string()
}

pub fn render_lint(document: &TestDocument) -> String {
    let mut out = Lines::new();
    out.title("twin test lint");
    out.status_row(Status::Ok, "file", "valid");
    out.blank();
    out.section("suite");
    out.tree_leaf(true, "name", &document.name);
    out.blank();
    out.section("checks");
    let last = document.checks.len().saturating_sub(1);
    for (index, check) in document.checks.iter().enumerate() {
        out.tree_leaf(index == last, check_label(check), &check.name);
    }
    out.into_string()
}

pub fn render_run(report: &TestRunReport) -> String {
    let mut out = Lines::new();
    out.title("twin test");
    let status = if report.failed > 0 {
        Status::Bad
    } else if report.warned > 0 {
        Status::Warn
    } else {
        Status::Ok
    };
    out.status_row(status, "suite", &report.name);
    out.blank();
    out.section(&format!("Twin Test: {}", report.name));
    for check in &report.checks {
        out.tree_leaf(false, status_word(check.status), &check.name);
        if let Some(detail) = &check.detail {
            out.tree_leaf(false, "detail", detail);
        }
        if let Some(evidence) = &check.evidence {
            out.tree_leaf(false, "evidence", evidence);
        }
    }
    out.blank();
    out.section("summary");
    out.tree_leaf(false, "passed", &report.passed.to_string());
    out.tree_leaf(false, "warned", &report.warned.to_string());
    out.tree_leaf(true, "failed", &report.failed.to_string());
    out.into_string()
}

fn status_word(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Pass => "PASS",
        CheckStatus::Warn => "WARN",
        CheckStatus::Fail => "FAIL",
    }
}

fn check_label(check: &twin_app::TestCheck) -> &'static str {
    use twin_app::CheckKind;
    match check.kind {
        CheckKind::Node(_) => "node",
        CheckKind::Port(_) => "port",
        CheckKind::Service(_) => "service",
        CheckKind::Dependency { .. } => "dependency",
        CheckKind::Emulate { .. } => "emulate",
        CheckKind::Outbound { .. } => "endpoints",
        CheckKind::Disk { .. } => "disk",
        CheckKind::Unknowns { .. } => "unknowns",
    }
}
