use twin_app::{
    WhatChangedEdge, WhatChangedEdgeDelta, WhatChangedNode, WhatChangedNodeDelta, WhatChangedResult,
};

use crate::output::format::{Lines, Status};
use crate::output::sections::{CHANGED, DISAPPEARED, NEW, REAPPEARED, STALE, UNKNOWNS};

const MAX_ITEMS: usize = 18;

#[derive(Debug, Clone, Copy)]
pub struct WhatChangedRenderOptions {
    pub verbose: bool,
}

impl WhatChangedRenderOptions {
    pub fn concise() -> Self {
        Self { verbose: false }
    }
}

pub fn render(result: &WhatChangedResult, options: WhatChangedRenderOptions) -> String {
    let mut out = Lines::new();
    out.title("twin what-changed");
    out.status_row(
        Status::Neutral,
        "window",
        &format!("since {}", result.since_label),
    );
    out.status_row(Status::Neutral, "summary", &summary_line(result));
    out.blank();
    render_nodes(&mut out, NEW, &result.new_nodes, options);
    render_nodes(&mut out, DISAPPEARED, &result.disappeared_nodes, options);
    render_nodes(&mut out, STALE, &result.stale_nodes, options);
    render_node_deltas(&mut out, CHANGED, &result.changed_nodes, options);
    render_nodes(&mut out, REAPPEARED, &result.reappeared_nodes, options);
    if has_edges(result) {
        out.blank();
        out.section("edges");
        render_edges(&mut out, NEW, &result.new_edges, options);
        render_edges(&mut out, DISAPPEARED, &result.disappeared_edges, options);
        render_edges(&mut out, STALE, &result.stale_edges, options);
        render_edge_deltas(&mut out, CHANGED, &result.changed_edges, options);
        render_edges(&mut out, REAPPEARED, &result.reappeared_edges, options);
    }
    if !result.unknowns.is_empty() {
        out.blank();
        out.section(UNKNOWNS);
        for (i, note) in result.unknowns.iter().enumerate() {
            out.tree_leaf(i + 1 == result.unknowns.len(), "note", note);
        }
    }
    out.into_string()
}

fn summary_line(result: &WhatChangedResult) -> String {
    let node_parts = section_counts(&[
        (NEW, result.new_nodes.len()),
        (DISAPPEARED, result.disappeared_nodes.len()),
        (STALE, result.stale_nodes.len()),
        (CHANGED, result.changed_nodes.len()),
        (REAPPEARED, result.reappeared_nodes.len()),
    ]);
    let edge_parts = section_counts(&[
        (NEW, result.new_edges.len()),
        (DISAPPEARED, result.disappeared_edges.len()),
        (STALE, result.stale_edges.len()),
        (CHANGED, result.changed_edges.len()),
        (REAPPEARED, result.reappeared_edges.len()),
    ]);
    match edge_parts.is_empty() {
        true => node_parts.join(" · "),
        false => format!(
            "{} (edges: {})",
            node_parts.join(" · "),
            edge_parts.join(" · ")
        ),
    }
}

fn section_counts(entries: &[(&str, usize)]) -> Vec<String> {
    entries
        .iter()
        .filter(|(_, count)| *count > 0)
        .map(|(label, count)| format!("{count} {label}"))
        .collect()
}

fn has_edges(result: &WhatChangedResult) -> bool {
    !result.new_edges.is_empty()
        || !result.disappeared_edges.is_empty()
        || !result.stale_edges.is_empty()
        || !result.changed_edges.is_empty()
        || !result.reappeared_edges.is_empty()
}

fn render_nodes(
    out: &mut Lines,
    title: &str,
    nodes: &[WhatChangedNode],
    options: WhatChangedRenderOptions,
) {
    if nodes.is_empty() {
        return;
    }
    out.section(title);
    out.tree_leaf(false, "kinds", &node_kind_counts(nodes));
    let mut sorted: Vec<_> = nodes.iter().collect();
    sort_nodes(&mut sorted);
    let (visible, hidden) = partition_by_limit(sorted.len(), options.verbose);
    for (i, node) in sorted.iter().take(visible).enumerate() {
        let is_last = i + 1 == visible && hidden == 0;
        out.tree_leaf(is_last, &node.kind, &format!("{}  {}", node.id, node.label));
    }
    render_more_line(out, hidden, options.verbose);
}

fn render_node_deltas(
    out: &mut Lines,
    title: &str,
    nodes: &[WhatChangedNodeDelta],
    options: WhatChangedRenderOptions,
) {
    if nodes.is_empty() {
        return;
    }
    out.section(title);
    out.tree_leaf(false, "kinds", &node_delta_kind_counts(nodes));
    let mut sorted: Vec<_> = nodes.iter().collect();
    sort_node_deltas(&mut sorted);
    let (visible, hidden) = partition_by_limit(sorted.len(), options.verbose);
    for (i, node) in sorted.iter().take(visible).enumerate() {
        let is_last = i + 1 == visible && hidden == 0;
        out.tree_leaf(
            is_last,
            &node.kind,
            &format!("{}  {}", node.id, node.summary),
        );
    }
    render_more_line(out, hidden, options.verbose);
}

fn render_edges(
    out: &mut Lines,
    title: &str,
    edges: &[WhatChangedEdge],
    options: WhatChangedRenderOptions,
) {
    if edges.is_empty() {
        return;
    }
    out.section(title);
    let (show, declared_hidden, other_hidden) = partition_edges(edges, options.verbose);
    if !options.verbose {
        out.tree_leaf(
            false,
            "kinds",
            &edge_kind_counts(edges, declared_hidden, other_hidden),
        );
    }
    let (visible, truncated) = partition_by_limit(show.len(), options.verbose);
    for (i, edge) in show.iter().take(visible).enumerate() {
        let is_last =
            i + 1 == visible && truncated == 0 && declared_hidden == 0 && other_hidden == 0;
        out.tree_leaf(is_last, &edge.kind, &format_edge(edge));
    }
    render_more_line(out, truncated, options.verbose);
    render_hidden_edge_line(
        out,
        declared_hidden,
        title,
        "declared depends_on",
        options.verbose,
    );
    render_hidden_edge_line(out, other_hidden, title, "other", options.verbose);
}

fn render_edge_deltas(
    out: &mut Lines,
    title: &str,
    edges: &[WhatChangedEdgeDelta],
    options: WhatChangedRenderOptions,
) {
    if edges.is_empty() {
        return;
    }
    out.section(title);
    let mut sorted: Vec<_> = edges.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let (show, hidden_declared, hidden_other) = if options.verbose {
        (sorted, 0, 0)
    } else {
        let mut visible = Vec::new();
        let mut declared = 0usize;
        let mut other = 0usize;
        for edge in sorted {
            match edge_visibility(&edge.id, None, None) {
                EdgeVisibility::Show => visible.push(edge),
                EdgeVisibility::HideDeclaredDependsOn => declared += 1,
                EdgeVisibility::HideLowSignal => other += 1,
            }
        }
        (visible, declared, other)
    };
    let (visible, truncated) = partition_by_limit(show.len(), options.verbose);
    for (i, edge) in show.iter().take(visible).enumerate() {
        let is_last =
            i + 1 == visible && truncated == 0 && hidden_declared == 0 && hidden_other == 0;
        out.tree_leaf(is_last, "edge", &format_edge_delta(edge));
    }
    render_more_line(out, truncated, options.verbose);
    render_hidden_edge_line(
        out,
        hidden_declared,
        title,
        "declared depends_on",
        options.verbose,
    );
    render_hidden_edge_line(out, hidden_other, title, "other", options.verbose);
}

fn partition_edges(
    edges: &[WhatChangedEdge],
    verbose: bool,
) -> (Vec<&WhatChangedEdge>, usize, usize) {
    if verbose {
        return (edges.iter().collect(), 0, 0);
    }
    let mut show = Vec::new();
    let mut declared = 0usize;
    let mut other = 0usize;
    for edge in edges {
        match edge_visibility(&edge.id, Some(&edge.kind), Some(&edge.class)) {
            EdgeVisibility::Show => show.push(edge),
            EdgeVisibility::HideDeclaredDependsOn => declared += 1,
            EdgeVisibility::HideLowSignal => other += 1,
        }
    }
    sort_edges(&mut show);
    (show, declared, other)
}

enum EdgeVisibility {
    Show,
    HideDeclaredDependsOn,
    HideLowSignal,
}

fn edge_visibility(id: &str, kind: Option<&str>, class: Option<&str>) -> EdgeVisibility {
    let kind = kind.unwrap_or_else(|| parse_edge_kind(id));
    if kind == "depends_on" && class == Some("observed") {
        return EdgeVisibility::HideDeclaredDependsOn;
    }
    if is_high_signal_edge_kind(kind) {
        EdgeVisibility::Show
    } else {
        EdgeVisibility::HideLowSignal
    }
}

fn parse_edge_kind(id: &str) -> &str {
    parse_edge_triple(id).map(|(_, kind, _)| kind).unwrap_or("")
}

fn is_high_signal_edge_kind(kind: &str) -> bool {
    matches!(
        kind,
        "listens_on"
            | "connects_to"
            | "owns"
            | "configured_by"
            | "depends_on"
            | "proxies_to"
            | "references"
    )
}

fn format_edge(edge: &WhatChangedEdge) -> String {
    format!(
        "{} → {} → {}",
        edge.from_node_id, edge.kind, edge.to_node_id
    )
}

fn format_edge_delta(edge: &WhatChangedEdgeDelta) -> String {
    match parse_edge_triple(&edge.id) {
        Some((from, kind, to)) => format!("{from} → {kind} → {to}  {}", edge.summary),
        None => format!("{}  {}", edge.id, edge.summary),
    }
}

fn parse_edge_triple(id: &str) -> Option<(&str, &str, &str)> {
    let mut parts = id.splitn(3, '|');
    let from = parts.next()?;
    let kind = parts.next()?;
    let to = parts.next()?;
    Some((from, kind, to))
}

fn node_priority(kind: &str) -> u8 {
    match kind {
        "process" => 0,
        "service" => 1,
        "port" => 2,
        "file" => 3,
        _ => 4,
    }
}

fn sort_nodes(nodes: &mut [&WhatChangedNode]) {
    nodes.sort_by(|a, b| {
        node_priority(&a.kind)
            .cmp(&node_priority(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn sort_node_deltas(nodes: &mut [&WhatChangedNodeDelta]) {
    nodes.sort_by(|a, b| {
        node_priority(&a.kind)
            .cmp(&node_priority(&b.kind))
            .then_with(|| a.id.cmp(&b.id))
    });
}

fn sort_edges(edges: &mut [&WhatChangedEdge]) {
    edges.sort_by(|a, b| a.id.cmp(&b.id));
}

fn node_kind_counts(nodes: &[WhatChangedNode]) -> String {
    kind_count_line(nodes.iter().map(|n| n.kind.as_str()))
}

fn node_delta_kind_counts(nodes: &[WhatChangedNodeDelta]) -> String {
    kind_count_line(nodes.iter().map(|n| n.kind.as_str()))
}

fn edge_kind_counts(
    edges: &[WhatChangedEdge],
    declared_hidden: usize,
    other_hidden: usize,
) -> String {
    let mut parts: Vec<String> = kind_count_parts(edges.iter().map(|e| e.kind.as_str()));
    if declared_hidden > 0 {
        parts.push(format!("depends_on edge ({declared_hidden} hidden)"));
    }
    if other_hidden > 0 {
        parts.push(format!("other ({other_hidden} hidden)"));
    }
    parts.join(" · ")
}

fn kind_count_line<'a, I>(kinds: I) -> String
where
    I: Iterator<Item = &'a str>,
{
    kind_count_parts(kinds).join(" · ")
}

fn kind_count_parts<'a, I>(kinds: I) -> Vec<String>
where
    I: Iterator<Item = &'a str>,
{
    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for kind in kinds {
        *counts.entry(kind).or_default() += 1;
    }
    counts
        .into_iter()
        .map(|(kind, count)| format!("{kind} ({count})"))
        .collect()
}

fn partition_by_limit(total: usize, verbose: bool) -> (usize, usize) {
    if verbose || total <= MAX_ITEMS {
        (total, 0)
    } else {
        (MAX_ITEMS, total - MAX_ITEMS)
    }
}

fn render_more_line(out: &mut Lines, hidden: usize, verbose: bool) {
    if hidden == 0 || verbose {
        return;
    }
    out.tree_leaf(true, "", &format!("… and {hidden} more (use --json)"));
}

fn render_hidden_edge_line(
    out: &mut Lines,
    hidden: usize,
    section: &str,
    label: &str,
    verbose: bool,
) {
    if hidden == 0 || verbose {
        return;
    }
    out.tree_leaf(
        true,
        "",
        &format!("{hidden} {label} edges {section} (hidden — use --json for full list)"),
    );
}
