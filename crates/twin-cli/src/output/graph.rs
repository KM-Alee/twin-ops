use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use twin_app::{GraphListResult, GraphNodeResult, GraphParentEdge, GraphResult};

pub fn render(result: &GraphResult) -> String {
    match result {
        GraphResult::List(list) => render_list(list),
        GraphResult::Node(node) => render_node(node),
    }
}

fn render_list(list: &GraphListResult) -> String {
    let title = match list.kind {
        twin_core::NodeKind::Process => "Process tree",
        other => return format!("Unsupported list kind: {other}"),
    };
    let mut lines = vec![format!("{title} ({} nodes)", list.nodes.len()), String::new()];

    let labels: HashMap<String, String> = list
        .nodes
        .iter()
        .map(|n| (n.id.clone(), n.label.clone()))
        .collect();
    let children = build_children_map(&list.parent_edges);
    let child_ids: HashSet<String> = list
        .parent_edges
        .iter()
        .map(|e| e.child_id.clone())
        .collect();
    let mut roots: Vec<String> = list
        .nodes
        .iter()
        .filter(|n| !child_ids.contains(&n.id))
        .map(|n| n.id.clone())
        .collect();
    roots.sort_by_key(|id| peer_pid(id).unwrap_or(u32::MAX));

    if roots.is_empty() {
        for node in &list.nodes {
            render_tree_branch(&mut lines, &node.id, &labels[&node.id], &children, &labels, "", true);
        }
    } else {
        for (i, root) in roots.iter().enumerate() {
            let is_last_root = i + 1 == roots.len();
            let label = labels.get(root).map(String::as_str).unwrap_or("?");
            render_tree_branch(&mut lines, root, label, &children, &labels, "", is_last_root);
            if !is_last_root {
                lines.push(String::new());
            }
        }
    }

    lines.join("\n")
}

fn render_node(node: &GraphNodeResult) -> String {
    let mut lines = Vec::new();

    lines.push("parent_of (observed)".to_string());
    lines.push(String::new());

    if node.incoming.is_empty() {
        lines.push("       (no parents in graph)".to_string());
        lines.push("              │".to_string());
    } else {
        let mut parents = node.incoming.clone();
        parents.sort_by_key(|e| peer_pid(&e.peer_id).unwrap_or(u32::MAX));
        for (i, edge) in parents.iter().enumerate() {
            let is_last = i + 1 == parents.len();
            lines.push(format!(
                "       {}── {}",
                if is_last { "└" } else { "├" },
                format_peer(&edge.peer_id, &edge.peer_label)
            ));
        }
        lines.push("              │".to_string());
    }

    lines.push(format!(
        "              ▼  {}",
        format_peer(&node.node.id, &node.node.label)
    ));
    lines.push("              │".to_string());

    if node.outgoing.is_empty() {
        lines.push("              └── (no children)".to_string());
    } else {
        let mut children = node.outgoing.clone();
        children.sort_by_key(|e| peer_pid(&e.peer_id).unwrap_or(u32::MAX));
        for (i, edge) in children.iter().enumerate() {
            let is_last = i + 1 == children.len();
            let connector = if is_last { "└──" } else { "├──" };
            lines.push(format!(
                "              {connector} {}",
                format_peer(&edge.peer_id, &edge.peer_label)
            ));
        }
    }

    if let Some(evidence) = render_evidence(&node.evidence_refs, node.outgoing.len()) {
        lines.push(String::new());
        lines.push("evidence".to_string());
        lines.extend(evidence);
    }

    lines.join("\n")
}

fn render_tree_branch(
    lines: &mut Vec<String>,
    node_id: &str,
    label: &str,
    children: &BTreeMap<String, Vec<(String, String)>>,
    labels: &HashMap<String, String>,
    prefix: &str,
    is_last: bool,
) {
    let branch = if is_last { "└──" } else { "├──" };
    lines.push(format!(
        "{prefix}{branch} {}",
        format_peer(node_id, label)
    ));
    let child_prefix = format!("{}{}", prefix, if is_last { "    " } else { "│   " });
    if let Some(kids) = children.get(node_id) {
        for (i, (child_id, child_label)) in kids.iter().enumerate() {
            let child_is_last = i + 1 == kids.len();
            let label = labels
                .get(child_id)
                .map(String::as_str)
                .unwrap_or(child_label.as_str());
            render_tree_branch(
                lines,
                child_id,
                label,
                children,
                labels,
                &child_prefix,
                child_is_last,
            );
        }
    }
}

fn build_children_map(edges: &[GraphParentEdge]) -> BTreeMap<String, Vec<(String, String)>> {
    let mut map: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for edge in edges {
        map.entry(edge.parent_id.clone())
            .or_default()
            .push((edge.child_id.clone(), edge.child_label.clone()));
    }
    for kids in map.values_mut() {
        kids.sort_by_key(|(id, _)| peer_pid(id).unwrap_or(u32::MAX));
    }
    map
}

fn format_peer(node_id: &str, label: &str) -> String {
    format!("{node_id}  {label}")
}

fn render_evidence(refs: &[String], child_count: usize) -> Option<Vec<String>> {
    if refs.is_empty() {
        return None;
    }
    let mut lines = Vec::new();
    if let Some(summary) = summarize_child_stat_evidence(refs, child_count) {
        lines.push(format!("  └─ {summary}"));
        return Some(lines);
    }
    let mut seen = BTreeSet::new();
    for line in refs {
        if seen.insert(line.clone()) {
            lines.push(format!("  └─ {line}"));
        }
    }
    if lines.is_empty() {
        None
    } else {
        Some(lines)
    }
}

fn summarize_child_stat_evidence(refs: &[String], child_count: usize) -> Option<String> {
    if refs.is_empty() || child_count == 0 {
        return None;
    }
    let mut by_ppid: BTreeMap<String, usize> = BTreeMap::new();
    let mut stat_lines = 0usize;
    for r in refs {
        if let Some(ppid) = parse_stat_ppid(r) {
            stat_lines += 1;
            *by_ppid.entry(ppid).or_default() += 1;
        }
    }
    if stat_lines == 0 {
        return None;
    }
    if by_ppid.len() == 1 {
        let (ppid, count) = by_ppid.into_iter().next()?;
        if count >= 2 {
            return Some(format!(
                "{count} children via /proc/<pid>/stat (ppid={ppid})"
            ));
        }
    }
    None
}

fn parse_stat_ppid(line: &str) -> Option<String> {
    let (_, ppid) = line.rsplit_once(" ppid=")?;
    if ppid.chars().all(|c| c.is_ascii_digit()) {
        Some(ppid.to_string())
    } else {
        None
    }
}

fn peer_pid(node_id: &str) -> Option<u32> {
    node_id.strip_prefix("process:pid:")?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_duplicate_ppid_evidence() {
        let refs: Vec<String> = (2..=5)
            .map(|n| format!("/proc/{n}/stat ppid=1"))
            .collect();
        let summary = summarize_child_stat_evidence(&refs, 4).expect("summary");
        assert!(summary.contains("4 children"));
        assert!(summary.contains("ppid=1"));
    }

    #[test]
    fn tree_branch_renders_connector() {
        let mut lines = Vec::new();
        let children = BTreeMap::new();
        let labels = HashMap::from([(
            "process:pid:1".to_string(),
            "systemd".to_string(),
        )]);
        render_tree_branch(
            &mut lines,
            "process:pid:1",
            "systemd",
            &children,
            &labels,
            "",
            true,
        );
        assert!(lines[0].contains("└──"));
        assert!(lines[0].contains("systemd"));
    }
}
