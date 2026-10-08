use std::collections::HashSet;

use twin_app::{ImpactDependent, ImpactEvidenceLine, ImpactPath, ImpactResult};
use twin_core::RiskLevel;

use crate::output::format::{evidence_label_words, Lines, ScanFreshness, Status};
use crate::output::sections::{
    CONFIGURED_DEPENDENTS, DIRECT_DEPENDENTS_RUNTIME, EVIDENCE, EVIDENCE_STRENGTH, IMPACT_PATHS,
    OWNED_BY, RISK, SCAN_HEALTH, TARGET, UNKNOWNS,
};
use crate::output::unknowns::{group_unknown_refs, render_unknown_groups, split_scan_health};

pub fn render(result: &ImpactResult) -> String {
    render_with_scan(result, None)
}

pub fn render_with_scan(result: &ImpactResult, scan: Option<ScanFreshness>) -> String {
    if result.target.starts_with("package:") {
        return render_package_impact(result, scan);
    }
    let mut out = Lines::new();
    out.title("twin impact");
    let status = impact_status(result);
    out.status_row(status, "summary", &impact_summary(result));
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(status, "view", "impact report");
    out.blank();
    out.section(TARGET);
    out.tree_leaf(true, &result.target_label, &result.target);
    out.blank();
    out.section(RISK);
    out.tree_leaf(false, "level", &result.risk.level.to_string());
    out.tree_leaf(false, EVIDENCE_STRENGTH, &result.evidence_strength.label);
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
        out.section(OWNED_BY);
        for (i, owner) in result.listener_owners.iter().enumerate() {
            let is_last = i + 1 == result.listener_owners.len();
            out.tree_leaf(
                is_last,
                &owner.label,
                &format!("{} ({})", owner.id, owner.edge_class),
            );
        }
    }
    out.blank();
    out.section(DIRECT_DEPENDENTS_RUNTIME);
    render_dependents(&mut out, &result.direct_dependents, true);
    if !result.configured_dependents.is_empty() {
        out.blank();
        out.section(CONFIGURED_DEPENDENTS);
        render_dependents(&mut out, &result.configured_dependents, true);
    }
    if result.paths_requested {
        out.blank();
        out.section(IMPACT_PATHS);
        render_impact_paths(&mut out, &result.impact_paths);
    }
    let evidence_lines: Vec<_> = if result.evidence.is_empty() {
        aggregate_dependent_evidence(result)
    } else {
        result.evidence.iter().collect()
    };
    if !evidence_lines.is_empty() {
        out.blank();
        out.section(EVIDENCE);
        for (i, line) in evidence_lines.iter().enumerate() {
            let is_last = i + 1 == evidence_lines.len();
            out.tree_leaf(is_last, &line.source, &line.statement);
        }
    }
    if result.show_evidence {
        render_evidence_score(
            &mut out,
            result.evidence_strength.score,
            &result.evidence_strength.label,
            &result.evidence_reasons,
        );
    }
    let (health, unknowns) = split_scan_health(&result.unknowns);
    if !health.is_empty() {
        out.blank();
        out.section(SCAN_HEALTH);
        for (i, note) in health.iter().enumerate() {
            let is_last = i + 1 == health.len();
            out.tree_leaf(is_last, "note", &note.detail);
        }
    }
    if !unknowns.is_empty() {
        out.blank();
        let weakens = unknowns.iter().any(|u| u.weakens_evidence);
        let title = if weakens {
            format!("{UNKNOWNS} (weakens evidence)")
        } else {
            UNKNOWNS.to_string()
        };
        out.section(&title);
        let grouped = group_unknown_refs(unknowns.iter().copied());
        render_unknown_groups(&mut out, &grouped);
    }
    out.into_string()
}

fn render_evidence_score(out: &mut Lines, score: u8, label: &str, reasons: &[String]) {
    out.blank();
    out.section("evidence score");
    let headline = format!("{score}/100, {}", evidence_label_words(label));
    if reasons.is_empty() {
        out.tree_leaf(true, "strength", &headline);
        return;
    }
    out.tree_leaf(false, "strength", &headline);
    for (index, reason) in reasons.iter().enumerate() {
        out.tree_leaf(index + 1 == reasons.len(), "why", reason);
    }
}

fn render_impact_paths(out: &mut Lines, paths: &[ImpactPath]) {
    if paths.is_empty() {
        out.tree_leaf(true, "(none)", "no dependency paths in graph");
        return;
    }
    let total = paths.len();
    for (i, path) in paths.iter().enumerate() {
        let is_last = i + 1 == total;
        let chain = path_chain_label(path);
        out.tree_branch("", !is_last, &path.terminal.label, &path.terminal.id);
        let indent = Lines::child_indent("", is_last);
        let mut rows: Vec<(&str, String)> =
            vec![("path", chain), ("depth", path.depth.to_string())];
        for line in &path.evidence {
            rows.push(("evidence", line.statement.clone()));
        }
        if path.is_depth_capped {
            rows.push(("note", format!("depth capped at {}", path.depth)));
        }
        if path.is_cycle_capped {
            let note = path
                .cycle_note
                .as_deref()
                .unwrap_or("cycle capped on path")
                .to_string();
            rows.push(("note", note));
        }
        for (i, (label, value)) in rows.iter().enumerate() {
            out.tree_branch(&indent, i + 1 == rows.len(), label, value);
        }
    }
}

fn path_chain_label(path: &ImpactPath) -> String {
    if path.steps.is_empty() {
        return path.terminal.id.clone();
    }
    let mut nodes = vec![path.terminal.id.clone()];
    for step in &path.steps {
        nodes.push(step.to.id.clone());
    }
    nodes.join(" -> ")
}

fn render_package_impact(result: &ImpactResult, scan: Option<ScanFreshness>) -> String {
    let mut out = Lines::new();
    out.title("twin impact");
    let status = impact_status(result);
    out.status_row(
        status,
        "summary",
        &format!(
            "{} risk · {} service(s) affected on restart",
            result.risk.level,
            result.configured_dependents.len()
        ),
    );
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(status, "view", "package restart blast radius");
    out.blank();
    out.section(TARGET);
    out.tree_leaf(true, &result.target_label, &result.target);
    out.blank();
    out.section(RISK);
    out.tree_leaf(false, "level", &result.risk.level.to_string());
    out.tree_leaf(true, EVIDENCE_STRENGTH, &result.evidence_strength.label);
    out.blank();
    out.section("restart blast radius");
    if result.configured_dependents.is_empty() {
        out.tree_leaf(true, "(none)", "no services depend on this package");
    } else {
        for (index, dependent) in result.configured_dependents.iter().enumerate() {
            let is_last = index + 1 == result.configured_dependents.len();
            out.tree_leaf(is_last, &dependent.label, &dependent.reason);
        }
    }
    out.into_string()
}

fn impact_summary(result: &ImpactResult) -> String {
    let mut parts = vec![
        format!("{} risk", result.risk.level),
        format!("{} runtime dependent(s)", result.direct_dependents.len()),
    ];
    if result.paths_requested {
        let transitive = result
            .impact_paths
            .iter()
            .filter(|p| p.depth >= 2)
            .map(|p| p.terminal.id.as_str())
            .collect::<HashSet<_>>()
            .len();
        if transitive > 0 {
            parts.push(format!("{transitive} transitive dependent(s)"));
        }
    }
    if !result.configured_dependents.is_empty() {
        parts.push(format!(
            "{} configured-only dependent(s)",
            result.configured_dependents.len()
        ));
    }
    if result.unknowns.iter().any(|u| u.weakens_evidence) {
        parts.push("scan degraded".to_string());
    }
    parts.join(" · ")
}

fn render_dependents(out: &mut Lines, dependents: &[ImpactDependent], allow_none: bool) {
    if dependents.is_empty() {
        if allow_none {
            out.tree_leaf(true, "(none)", "no runtime dependents in graph");
        }
        return;
    }
    let total = dependents.len();
    for (i, dependent) in dependents.iter().enumerate() {
        let is_last_dep = i + 1 == total;
        let detail = format!(
            "relationship: {} {} ({})",
            dependent.relationship, dependent.edge_class, dependent.impact_kind
        );
        let evidence_count = dependent.evidence.len();
        let has_children = true;
        out.tree_branch("", !is_last_dep || has_children, &dependent.label, &detail);
        let indent = Lines::child_indent("", is_last_dep);
        out.tree_branch(&indent, evidence_count == 0, "reason", &dependent.reason);
        for (j, line) in dependent.evidence.iter().enumerate() {
            let is_last_ev = j + 1 == evidence_count;
            out.tree_branch(&indent, is_last_ev, "evidence", &line.statement);
        }
    }
}

fn aggregate_dependent_evidence(result: &ImpactResult) -> Vec<&ImpactEvidenceLine> {
    let mut seen = HashSet::new();
    let mut lines = Vec::new();
    for dependent in result
        .direct_dependents
        .iter()
        .chain(result.configured_dependents.iter())
    {
        for line in &dependent.evidence {
            let key = format!("{}|{}", line.source, line.statement);
            if seen.insert(key) {
                lines.push(line);
            }
        }
    }
    lines
}

fn impact_status(result: &ImpactResult) -> Status {
    if result.unknowns.iter().any(|u| u.weakens_evidence) {
        return Status::Warn;
    }
    match result.risk.level {
        RiskLevel::High | RiskLevel::Critical | RiskLevel::Unknown => Status::Warn,
        RiskLevel::Low | RiskLevel::Medium => Status::Ok,
    }
}
