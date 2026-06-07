use twin_app::DiffResult;

use crate::output::format::Lines;
use crate::output::sections::{ADDED, CHANGED, REMOVED};

pub fn render(result: &DiffResult) -> String {
    let mut out = Lines::new();
    out.title("twin diff");
    out.status_row(
        crate::output::format::Status::Neutral,
        "view",
        &format!("{} -> {}", result.left_ref, result.right_ref),
    );
    out.blank();
    render_nodes(&mut out, result);
    render_edges(&mut out, result);
    if result.nodes_added.is_empty()
        && result.nodes_removed.is_empty()
        && result.nodes_changed.is_empty()
        && result.edges_added.is_empty()
        && result.edges_removed.is_empty()
        && result.edges_changed.is_empty()
    {
        out.status_row(
            crate::output::format::Status::Ok,
            "changes",
            "no differences",
        );
    }
    out.into_string()
}

fn render_nodes(out: &mut Lines, result: &DiffResult) {
    render_group(
        out,
        ADDED,
        &result
            .nodes_added
            .iter()
            .map(|n| format!("{} {}", n.id, n.label))
            .collect::<Vec<_>>(),
    );
    render_group(
        out,
        REMOVED,
        &result
            .nodes_removed
            .iter()
            .map(|n| format!("{} {}", n.id, n.label))
            .collect::<Vec<_>>(),
    );
    render_group(
        out,
        CHANGED,
        &result
            .nodes_changed
            .iter()
            .map(|n| format!("{} {}", n.id, n.summary))
            .collect::<Vec<_>>(),
    );
}

fn render_edges(out: &mut Lines, result: &DiffResult) {
    if result.edges_added.is_empty()
        && result.edges_removed.is_empty()
        && result.edges_changed.is_empty()
    {
        return;
    }
    out.blank();
    out.section("edges");
    render_group(
        out,
        ADDED,
        &result
            .edges_added
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
    );
    render_group(
        out,
        REMOVED,
        &result
            .edges_removed
            .iter()
            .map(|e| e.id.clone())
            .collect::<Vec<_>>(),
    );
    render_group(
        out,
        CHANGED,
        &result
            .edges_changed
            .iter()
            .map(|e| format!("{} {}", e.id, e.summary))
            .collect::<Vec<_>>(),
    );
}

fn render_group(out: &mut Lines, title: &str, items: &[String]) {
    if items.is_empty() {
        return;
    }
    out.section(title);
    for (i, item) in items.iter().enumerate() {
        out.tree_leaf(i + 1 == items.len(), "", item);
    }
}
