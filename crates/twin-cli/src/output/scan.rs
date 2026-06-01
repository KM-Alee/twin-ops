use twin_app::ScanResult;
use twin_collectors::ProcessWarningKind;

use crate::output::format::{format_duration_ns, Lines, Status};

pub fn render(result: &ScanResult) -> String {
    let mut out = Lines::new();
    out.title("twin scan");

    let status = if result.warning_count > 0 {
        Status::Warn
    } else {
        Status::Ok
    };
    out.status_row(
        status,
        "result",
        &format!(
            "{} processes, {} services, {} parent edges",
            result.process_count, result.service_count, result.parent_edge_count
        ),
    );
    out.status_row(
        Status::Neutral,
        "duration",
        &format_duration_ns(result.started_at_ns, result.ended_at_ns),
    );
    out.blank();
    out.section("persisted");
    out.tree_leaf(false, "processes", &result.process_count.to_string());
    out.tree_leaf(false, "cgroups", &result.cgroup_count.to_string());
    out.tree_leaf(false, "services", &result.service_count.to_string());
    out.tree_leaf(false, "parent-of", &result.parent_edge_count.to_string());
    out.tree_leaf(false, "in-cgroup", &result.in_cgroup_edge_count.to_string());
    out.tree_leaf(
        false,
        "service-owns",
        &result.service_owns_edge_count.to_string(),
    );
    out.tree_leaf(true, "observations", &result.observation_count.to_string());

    if result.warning_count > 0 {
        out.blank();
        out.section(&format!("warnings ({})", result.warning_count));
        let warnings = &result.warnings;
        for (i, warning) in warnings.iter().enumerate() {
            let is_last = i + 1 == warnings.len();
            let (kind, detail) = warning_parts(&warning.kind, warning.count);
            out.tree_leaf(is_last, kind, &detail);
        }
    }

    out.into_string()
}

fn warning_parts(kind: &str, count: usize) -> (&'static str, String) {
    if let Some(kind) = ProcessWarningKind::from_aggregate_key(kind) {
        return kind.cli_summary(count);
    }
    ("other", format!("{count} {}", kind.replace('_', " ")))
}
