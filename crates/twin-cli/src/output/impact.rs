use twin_app::ImpactResult;

use crate::output::format::{Lines, Status};

pub fn render(result: &ImpactResult) -> String {
    let mut out = Lines::new();
    out.title("twin impact");
    out.status_row(Status::Ok, "view", "direct dependents");
    out.blank();
    out.section("target");
    out.tree_leaf(true, &result.target, &result.target_label);
    out.blank();
    out.section("direct dependents");
    if result.direct_dependents.is_empty() {
        out.tree_leaf(true, "(none)", "no direct dependents in graph");
    } else {
        for (i, dependent) in result.direct_dependents.iter().enumerate() {
            let is_last = i + 1 == result.direct_dependents.len();
            let detail = format!(
                "{} {}  relationship: {} {}",
                dependent.id, dependent.label, dependent.relationship, dependent.edge_class
            );
            out.tree_leaf(is_last, "dependent", &detail);
        }
    }
    if !result.evidence.is_empty() {
        out.blank();
        out.section("evidence");
        for (i, line) in result.evidence.iter().enumerate() {
            let is_last = i + 1 == result.evidence.len();
            out.tree_leaf(is_last, &line.source, &line.statement);
        }
    }
    out.into_string()
}
