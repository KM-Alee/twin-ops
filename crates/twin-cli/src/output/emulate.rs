use std::collections::{BTreeMap, HashSet};

use twin_app::{EmulationImpact, EmulationImpactPathView, EmulationOverlayNode, EmulationResult};
use twin_core::RiskLevel;

use crate::output::format::{evidence_label_words, Lines, ScanFreshness, Status};
use crate::output::sections::{
    emulation_scoring_note, CONFIGURED_CONTEXT, EVIDENCE, EVIDENCE_STRENGTH, IMPACT_PATHS, OVERLAY,
    OVERLAY_SERVICE, OVERLAY_TCP_LISTENERS, OVERLAY_UNIX_LISTENERS, PERSISTENT_IMPACT,
    RESTART_IMPACT, RISK, RUNTIME_IMPACT, SAFETY, SCAN_HEALTH, TARGET, TRANSIENT_IMPACT, UNKNOWNS,
    UNKNOWN_IMPACT,
};
use crate::output::unknowns::{group_unknown_refs, render_unknown_groups, split_scan_health};

pub fn render(result: &EmulationResult) -> String {
    render_with_scan(result, None)
}

pub fn render_with_scan(result: &EmulationResult, scan: Option<ScanFreshness>) -> String {
    if result.action == "delete" {
        return render_delete_with_scan(result, scan);
    }
    if result.action == "fill-disk" {
        return render_fill_with_scan(result, scan);
    }
    let mut out = Lines::new();
    out.title(&format!("twin emulate restart {}", result.target_label));
    let status = emulate_status(result);
    out.status_row(status, "summary", &emulate_summary(result));
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(status, "view", "emulation report");
    out.blank();
    out.section(TARGET);
    out.tree_leaf(true, &result.target_label, &result.target);
    out.blank();
    out.section(RISK);
    out.tree_leaf(false, "level", &result.risk.level.to_string());
    out.tree_leaf(false, EVIDENCE_STRENGTH, &result.evidence_strength.label);
    if result.risk.reasons.is_empty() {
        out.tree_leaf(false, "reason", "no scoring reasons recorded");
    } else {
        for reason in &result.risk.reasons {
            out.tree_leaf(false, "reason", reason);
        }
    }
    out.tree_leaf(true, "note", emulation_scoring_note(result.paths_requested));
    render_overlay(&mut out, result);
    render_impact_section(&mut out, TRANSIENT_IMPACT, &result.transient_impacts);
    render_impact_section(&mut out, CONFIGURED_CONTEXT, &result.configured_impacts);
    if result.paths_requested {
        out.blank();
        out.section(IMPACT_PATHS);
        render_emulation_impact_paths(&mut out, &result.impact_paths);
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
    out.blank();
    out.section(SAFETY);
    out.tree_leaf(true, "status", &result.safety_statement);
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

fn render_delete_with_scan(result: &EmulationResult, scan: Option<ScanFreshness>) -> String {
    let mut out = Lines::new();
    out.title(&format!("Emulation: delete {}", result.target));
    let status = emulate_status(result);
    out.status_row(status, "summary", &delete_emulate_summary(result));
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(status, "view", "emulation report");
    out.blank();
    out.section(RISK);
    out.tree_leaf(false, "level", &result.risk.level.to_string());
    out.tree_leaf(true, EVIDENCE_STRENGTH, &result.evidence_strength.label);
    render_delete_overlay(&mut out, result);
    render_impact_section(&mut out, RUNTIME_IMPACT, &result.runtime_impacts);
    render_impact_section(&mut out, RESTART_IMPACT, &result.restart_impacts);
    render_impact_section(&mut out, PERSISTENT_IMPACT, &result.persistent_impacts);
    render_impact_section(&mut out, UNKNOWN_IMPACT, &result.unknown_impacts);
    if !result.evidence_lines.is_empty() {
        out.blank();
        out.section(EVIDENCE);
        for (i, line) in result.evidence_lines.iter().enumerate() {
            let is_last = i + 1 == result.evidence_lines.len();
            out.tree_leaf(is_last, "source", line);
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
    out.blank();
    out.section(SAFETY);
    out.tree_leaf(false, "status", &result.safety_statement);
    out.tree_leaf(true, "status", &result.general_safety_statement);
    out.into_string()
}

fn render_fill_with_scan(result: &EmulationResult, scan: Option<ScanFreshness>) -> String {
    let mut out = Lines::new();
    let percent = result
        .risk
        .reasons
        .iter()
        .find_map(|reason| {
            reason
                .strip_prefix("hypothetical fill to ")
                .and_then(|rest| rest.strip_suffix('%'))
        })
        .unwrap_or("");
    let title_percent = if percent.is_empty() {
        result.target_label.clone()
    } else {
        format!("{} to {percent}%", result.target)
    };
    out.title(&format!("Emulation: fill {title_percent}"));
    let status = emulate_status(result);
    out.status_row(
        status,
        "summary",
        &format!(
            "{} risk · {} affected service(s)",
            result.risk.level,
            result.persistent_impacts.len()
        ),
    );
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(status, "view", "emulation report");
    out.blank();
    out.section(RISK);
    out.tree_leaf(false, "level", &result.risk.level.to_string());
    out.tree_leaf(true, EVIDENCE_STRENGTH, &result.evidence_strength.label);
    if result.persistent_impacts.is_empty() {
        out.blank();
        out.section("likely affected");
        out.tree_leaf(true, "services", "none recorded on this mount");
    } else {
        render_impact_section(&mut out, "likely affected", &result.persistent_impacts);
    }
    if !result.evidence_lines.is_empty() {
        out.blank();
        out.section(EVIDENCE);
        for (i, line) in result.evidence_lines.iter().enumerate() {
            let is_last = i + 1 == result.evidence_lines.len();
            out.tree_leaf(is_last, "source", line);
        }
    }
    out.blank();
    out.section(SAFETY);
    out.tree_leaf(false, "status", &result.safety_statement);
    out.tree_leaf(true, "status", &result.general_safety_statement);
    out.into_string()
}

fn delete_emulate_summary(result: &EmulationResult) -> String {
    format!(
        "{} risk · {} configured service(s)",
        result.risk.level,
        result.restart_impacts.len()
    )
}

fn render_delete_overlay(out: &mut Lines, result: &EmulationResult) {
    if result.overlay.unavailable_nodes.is_empty() {
        return;
    }
    out.blank();
    out.section(OVERLAY);
    for (i, node) in result.overlay.unavailable_nodes.iter().enumerate() {
        let is_last = i + 1 == result.overlay.unavailable_nodes.len();
        out.tree_leaf(is_last, &node.label, "hypothetically deleted");
    }
}

fn render_emulation_impact_paths(out: &mut Lines, paths: &[EmulationImpactPathView]) {
    if paths.is_empty() {
        out.tree_leaf(true, "(none)", "no dependency paths in graph");
        return;
    }
    let total = paths.len();
    for (i, path) in paths.iter().enumerate() {
        let is_last = i + 1 == total;
        out.tree_branch("", !is_last, &path.terminal_label, &path.terminal_id);
        let indent = Lines::child_indent("", is_last);
        let mut rows: Vec<(&str, String)> = vec![
            ("path", path.path_chain.clone()),
            ("depth", path.depth.to_string()),
        ];
        for line in &path.evidence {
            rows.push(("evidence", line.clone()));
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

fn emulate_summary(result: &EmulationResult) -> String {
    let mut parts = vec![
        format!("{} risk", result.risk.level),
        format!("{} runtime dependent(s)", result.transient_impacts.len()),
    ];
    if result.paths_requested {
        let transitive = result
            .impact_paths
            .iter()
            .filter(|p| p.depth >= 2)
            .map(|p| p.terminal_id.as_str())
            .collect::<HashSet<_>>()
            .len();
        if transitive > 0 {
            parts.push(format!("{transitive} transitive dependent(s)"));
        }
    }
    let listener_count = result
        .overlay
        .unavailable_nodes
        .iter()
        .filter(|node| node.id.starts_with("port:") || node.id.starts_with("unix:"))
        .count();
    if listener_count > 0 {
        parts.push(format!("{listener_count} listener(s) overlayed"));
    }
    if !result.configured_impacts.is_empty() {
        parts.push(format!(
            "{} configured-only dependent(s)",
            result.configured_impacts.len()
        ));
    }
    if result.unknowns.iter().any(|u| u.weakens_evidence) {
        parts.push("scan degraded".to_string());
    }
    parts.join(" · ")
}

fn render_overlay(out: &mut Lines, result: &EmulationResult) {
    if result.overlay.unavailable_nodes.is_empty() {
        return;
    }
    out.blank();
    out.section(OVERLAY);
    let mut groups: BTreeMap<&'static str, Vec<&EmulationOverlayNode>> = BTreeMap::new();
    for node in &result.overlay.unavailable_nodes {
        groups
            .entry(overlay_group(&node.id))
            .or_default()
            .push(node);
    }
    let group_keys: Vec<_> = groups.keys().copied().collect();
    for (gi, group_key) in group_keys.iter().enumerate() {
        let is_last_group = gi + 1 == group_keys.len();
        let nodes = &groups[group_key];
        out.section(group_key);
        for (i, node) in nodes.iter().enumerate() {
            let is_last = is_last_group && i + 1 == nodes.len();
            out.tree_leaf(is_last, &node.label, "temporarily unavailable");
        }
    }
}

fn overlay_group(id: &str) -> &'static str {
    if id.starts_with("service:") {
        OVERLAY_SERVICE
    } else if id.starts_with("port:") {
        OVERLAY_TCP_LISTENERS
    } else if id.starts_with("unix:") {
        OVERLAY_UNIX_LISTENERS
    } else {
        OVERLAY
    }
}

fn render_impact_section(out: &mut Lines, title: &str, impacts: &[EmulationImpact]) {
    if impacts.is_empty() {
        return;
    }
    out.blank();
    out.section(title);
    let total = impacts.len();
    for (i, impact) in impacts.iter().enumerate() {
        let is_last_imp = i + 1 == total;
        let child_count = usize::from(!impact.path.is_empty()) + impact.evidence.len();
        let has_children = child_count > 0;
        out.tree_branch(
            "",
            !is_last_imp || has_children,
            &impact.label,
            &impact.statement,
        );
        if !has_children {
            continue;
        }
        let indent = Lines::child_indent("", is_last_imp);
        let mut child_index = 0;
        if !impact.path.is_empty() {
            child_index += 1;
            let is_last_child = child_index == child_count;
            out.tree_branch(&indent, is_last_child, "path", &impact.path);
        }
        for line in &impact.evidence {
            child_index += 1;
            let is_last_child = child_index == child_count;
            out.tree_branch(&indent, is_last_child, "evidence", line);
        }
    }
}

fn emulate_status(result: &EmulationResult) -> Status {
    if result.unknowns.iter().any(|u| u.weakens_evidence) {
        return Status::Warn;
    }
    match result.risk.level {
        RiskLevel::High | RiskLevel::Critical | RiskLevel::Unknown => Status::Warn,
        RiskLevel::Low | RiskLevel::Medium => Status::Ok,
    }
}
