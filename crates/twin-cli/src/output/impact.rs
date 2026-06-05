use twin_app::ImpactResult;

use crate::output::format::{Lines, Status};

pub fn render(result: &ImpactResult) -> String {
    let mut out = Lines::new();
    out.title("twin impact");
    let status = impact_status(result);
    out.status_row(status, "view", "impact report");
    out.blank();
    out.section("target");
    out.tree_leaf(true, &result.target, &result.target_label);
    out.blank();
    out.section("risk");
    out.tree_leaf(false, "level", &result.risk.level);
    out.tree_leaf(false, "evidence strength", &result.evidence_strength.label);
    if result.risk.reasons.is_empty() {
        out.tree_leaf(true, "reason", "no scoring reasons recorded");
    } else {
        for (i, reason) in result.risk.reasons.iter().enumerate() {
            let is_last = i + 1 == result.risk.reasons.len();
            out.tree_leaf(is_last, "reason", reason);
        }
    }
    if !result.listener_owners.is_empty() {
        out.blank();
        out.section("owned by");
        for (i, owner) in result.listener_owners.iter().enumerate() {
            let is_last = i + 1 == result.listener_owners.len();
            let detail = format!("{} {}  {}", owner.id, owner.label, owner.edge_class);
            out.tree_leaf(is_last, "owner", &detail);
        }
    }
    out.blank();
    out.section("direct dependents (runtime)");
    if result.direct_dependents.is_empty() {
        out.tree_leaf(true, "(none)", "no runtime dependents in graph");
    } else {
        let total = result.direct_dependents.len();
        for (i, dependent) in result.direct_dependents.iter().enumerate() {
            let is_last_dep = i + 1 == total;
            out.tree_leaf(
                !is_last_dep,
                &dependent.id,
                &format!(
                    "{}  relationship: {} {} ({})",
                    dependent.label,
                    dependent.relationship,
                    dependent.edge_class,
                    dependent.impact_kind
                ),
            );
            out.tree_leaf(is_last_dep, "reason", &dependent.reason);
        }
    }
    if !result.configured_dependents.is_empty() {
        out.blank();
        out.section("configured dependents (inactive)");
        let total = result.configured_dependents.len();
        for (i, dependent) in result.configured_dependents.iter().enumerate() {
            let is_last_dep = i + 1 == total;
            out.tree_leaf(
                !is_last_dep,
                &dependent.id,
                &format!(
                    "{}  relationship: {} {} ({})",
                    dependent.label,
                    dependent.relationship,
                    dependent.edge_class,
                    dependent.impact_kind
                ),
            );
            out.tree_leaf(is_last_dep, "reason", &dependent.reason);
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
    let health: Vec<_> = result
        .unknowns
        .iter()
        .filter(|u| u.kind == "scan_health")
        .collect();
    let unknowns: Vec<_> = result
        .unknowns
        .iter()
        .filter(|u| u.kind != "scan_health")
        .collect();
    if !health.is_empty() {
        out.blank();
        out.section("scan health");
        for (i, note) in health.iter().enumerate() {
            let is_last = i + 1 == health.len();
            out.tree_leaf(is_last, "note", &note.detail);
        }
    }
    if !unknowns.is_empty() {
        out.blank();
        out.section("unknowns");
        for (i, unknown) in unknowns.iter().enumerate() {
            let is_last = i + 1 == unknowns.len();
            out.tree_leaf(is_last, &unknown.kind, &unknown.detail);
        }
    }
    out.into_string()
}

fn impact_status(result: &ImpactResult) -> Status {
    if result.unknowns.iter().any(|u| u.weakens_evidence) {
        return Status::Warn;
    }
    match result.risk.level.as_str() {
        "high" | "critical" | "unknown" => Status::Warn,
        _ => Status::Ok,
    }
}
