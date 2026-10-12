use twin_app::{PreflightResult, PreflightStep, PreflightVerdict};

use super::format::{Lines, Status};

pub fn render(result: &PreflightResult) -> String {
    let mut out = Lines::new();
    out.title("twin preflight");
    out.status_row(
        verdict_status(result.verdict),
        verdict_word(result.verdict),
        &result.headline,
    );
    out.status_row(Status::Neutral, "command", &result.command);
    if let Some(age) = result.graph_age_secs {
        out.status_row(Status::Neutral, "graph", &format!("scanned {age}s ago"));
    }
    for step in visible_steps(result) {
        out.blank();
        out.section(&format!(
            "{} · {}",
            verdict_word(step.verdict),
            step.operation
        ));
        render_step(&mut out, step);
    }
    if !result.coverage_notes.is_empty() {
        out.blank();
        out.section("coverage");
        let total = result.coverage_notes.len();
        for (index, note) in result.coverage_notes.iter().enumerate() {
            out.tree_leaf(index + 1 == total, "unknown", note);
        }
    }
    out.blank();
    out.section("safety");
    out.tree_leaf(
        true,
        "status",
        "Nothing was executed; this is a read-only prediction.",
    );
    out.into_string()
}

fn visible_steps(result: &PreflightResult) -> impl Iterator<Item = &PreflightStep> {
    let show_go = !result.has_operations;
    result
        .steps
        .iter()
        .filter(move |step| show_go || step.is_assessed || step.verdict != PreflightVerdict::Go)
}

fn render_step(out: &mut Lines, step: &PreflightStep) {
    let mut rows: Vec<(String, String)> = Vec::new();
    rows.push(("segment".into(), step.segment.clone()));
    if let Some(target) = &step.target {
        rows.push(("target".into(), target.clone()));
    }
    if let Some(risk) = step.risk {
        let evidence = step
            .evidence
            .as_ref()
            .map(|e| format!(" · evidence {} ({}/100)", e.label, e.score))
            .unwrap_or_default();
        rows.push(("risk".into(), format!("{risk}{evidence}")));
    } else if !step.is_assessed {
        rows.push(("assessed".into(), "no — not modeled in the graph".into()));
    }
    for reason in &step.reasons {
        rows.push(("reason".into(), reason.clone()));
    }
    for affected in &step.affected {
        rows.push((
            "affected".into(),
            format!("{} — {}", affected.label, affected.effect),
        ));
    }
    if step.affected_total > step.affected.len() {
        rows.push((
            "affected".into(),
            format!("… {} more", step.affected_total - step.affected.len()),
        ));
    }
    for advice in &step.advice {
        rows.push(("advice".into(), advice.clone()));
    }
    let total = rows.len();
    for (index, (label, value)) in rows.into_iter().enumerate() {
        out.tree_leaf(index + 1 == total, &label, &value);
    }
}

fn verdict_status(verdict: PreflightVerdict) -> Status {
    match verdict {
        PreflightVerdict::Go => Status::Ok,
        PreflightVerdict::Caution => Status::Warn,
        PreflightVerdict::Stop => Status::Bad,
    }
}

pub fn verdict_word(verdict: PreflightVerdict) -> &'static str {
    match verdict {
        PreflightVerdict::Go => "GO",
        PreflightVerdict::Caution => "CAUTION",
        PreflightVerdict::Stop => "STOP",
    }
}

/// Compact plain-text summary for agent hooks and MCP clients, bounded so it never
/// floods an agent's context.
pub fn render_brief(result: &PreflightResult) -> String {
    let mut lines = vec![format!(
        "twin preflight: {} — {}",
        verdict_word(result.verdict),
        result.headline
    )];
    for step in visible_steps(result).filter(|step| step.verdict != PreflightVerdict::Go) {
        lines.push(format!("* {} [{}]", step.operation, step.verdict.as_str()));
        for reason in step.reasons.iter().take(3) {
            lines.push(format!("  - {reason}"));
        }
        if !step.affected.is_empty() {
            let names = step
                .affected
                .iter()
                .take(5)
                .map(|a| a.label.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let more = step.affected_total.saturating_sub(5);
            let tail = if more > 0 {
                format!(" (+{more} more)")
            } else {
                String::new()
            };
            lines.push(format!("  - affected: {names}{tail}"));
        }
        for advice in step.advice.iter().take(2) {
            lines.push(format!("  - advice: {advice}"));
        }
    }
    if result.verdict != PreflightVerdict::Go && !result.coverage_notes.is_empty() {
        lines.push(format!(
            "coverage gaps: {}",
            result.coverage_notes.join("; ")
        ));
    }
    lines.push("(read-only prediction by twin; nothing was executed)".into());
    let mut text = lines.join("\n");
    if text.len() > 1800 {
        let cut = text
            .char_indices()
            .take_while(|(index, _)| *index < 1780)
            .last()
            .map(|(index, ch)| index + ch.len_utf8())
            .unwrap_or(0);
        text.truncate(cut);
        text.push_str("\n…");
    }
    text
}
