use std::path::Path;

use twin_app::{CheckStatus, TestDocument, TestInitOutcome, TestRunReport, TestRunSummary};

use crate::output::format::{format_age, Lines, Status};

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
    if let Some(context) = &report.context {
        out.status_row(
            Status::Neutral,
            "graph",
            &context
                .snapshot
                .as_ref()
                .map(|name| format!("snapshot:{name}"))
                .unwrap_or_else(|| "stored current graph".into()),
        );
        if let Some(observed) = context.graph_observed_at_ns {
            out.status_row(Status::Neutral, "graph observed", &format_age(observed));
        } else {
            out.status_row(
                Status::Warn,
                "graph observed",
                "no completed scan timestamp",
            );
        }
        if let Some(seconds) = context.max_age_seconds {
            out.status_row(
                Status::Neutral,
                "maximum evidence age",
                &format!("{seconds}s"),
            );
        }
        if !context.live_sources.is_empty() {
            out.status_row(
                Status::Neutral,
                "live sources at evaluation",
                &context.live_sources.join(", "),
            );
        }
    }
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
    if let Some(baseline) = &report.baseline {
        out.blank();
        out.section(&format!(
            "baseline comparison · snapshot:{}",
            baseline.snapshot
        ));
        for (label, names) in [
            ("newly failing", &baseline.newly_failing),
            ("newly uncertain", &baseline.newly_uncertain),
            ("recovered", &baseline.recovered),
        ] {
            out.tree_leaf(
                false,
                label,
                &if names.is_empty() {
                    "none".into()
                } else {
                    names.join(", ")
                },
            );
        }
        out.tree_leaf(true, "interpretation", "same checks evaluated against retained baseline graph; temporal difference does not establish cause");
    }
    out.into_string()
}

pub fn render_list(runs: &[TestRunSummary]) -> String {
    let mut out = Lines::new();
    out.title("twin test list");
    if runs.is_empty() {
        out.status_row(Status::Neutral, "runs", "none");
        return out.into_string();
    }
    out.status_row(Status::Ok, "runs", &format!("{} run(s)", runs.len()));
    out.blank();
    for run in runs {
        let status = if run.failed > 0 {
            Status::Bad
        } else if run.warned > 0 {
            Status::Warn
        } else {
            Status::Ok
        };
        let detail = format!(
            "{} · {} passed · {} warned · {} failed · {}",
            format_age(run.started_at_ns),
            run.passed,
            run.warned,
            run.failed,
            run.path
        );
        out.status_row(status, &run.name, &detail);
    }
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
        CheckKind::ServiceState { .. } => "service state",
        CheckKind::Listener { .. } => "listener ownership",
        CheckKind::ContainerHealth { .. } => "container health",
        CheckKind::K8sReady { .. } => "Kubernetes readiness",
        CheckKind::Dependency { .. } => "dependency",
        CheckKind::Emulate { .. } => "emulate",
        CheckKind::Outbound { .. } => "endpoints",
        CheckKind::Disk { .. } => "disk",
        CheckKind::Unknowns { .. } => "unknowns",
    }
}
