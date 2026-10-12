use twin_app::{BriefResult, PreflightVerdict};

use super::format::{Lines, Status};
use super::preflight::verdict_word;

pub fn render(result: &BriefResult) -> String {
    let mut out = Lines::new();
    out.title(&format!("twin brief · {}", result.title));
    out.status_row(Status::Neutral, "kind", &result.kind);
    for fact in &result.facts {
        out.status_row(Status::Neutral, &fact.label, &fact.value);
    }
    if let Some(restart) = &result.restart {
        let status = match restart.verdict {
            PreflightVerdict::Go => Status::Ok,
            PreflightVerdict::Caution => Status::Warn,
            PreflightVerdict::Stop => Status::Bad,
        };
        out.status_row(
            status,
            "if restarted",
            &format!("{} — {}", verdict_word(restart.verdict), restart.summary),
        );
    }
    out.status_row(
        Status::Neutral,
        "graph",
        &format!("scanned {}s ago", result.graph_age_secs),
    );
    for section in &result.sections {
        out.blank();
        out.section(section.title.as_str());
        let total = section.items.len() + usize::from(section.omitted > 0);
        for (index, item) in section.items.iter().enumerate() {
            let value = match (&item.id, item.detail.is_empty()) {
                (Some(id), true) => id.clone(),
                (Some(id), false) if id != &item.label && !id.ends_with(&item.label) => {
                    format!("{} · {id}", item.detail)
                }
                _ => item.detail.clone(),
            };
            out.tree_leaf(index + 1 == total, &item.label, &value);
        }
        if section.omitted > 0 {
            out.tree_leaf(true, "…", &format!("{} more", section.omitted));
        }
    }
    if !result.coverage_notes.is_empty() {
        out.blank();
        out.section("coverage");
        let total = result.coverage_notes.len();
        for (index, note) in result.coverage_notes.iter().enumerate() {
            out.tree_leaf(index + 1 == total, "unknown", note);
        }
    }
    if !result.next_steps.is_empty() {
        out.blank();
        out.section("next");
        let total = result.next_steps.len();
        for (index, step) in result.next_steps.iter().enumerate() {
            out.tree_leaf(index + 1 == total, "run", step);
        }
    }
    out.into_string()
}
