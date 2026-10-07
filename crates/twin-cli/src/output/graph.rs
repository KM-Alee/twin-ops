use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use twin_app::{
    GraphDirectoryResult, GraphFileResult, GraphListResult, GraphMountResult, GraphNodeResult,
    GraphOwnedNode, GraphParentEdge, GraphPortResult, GraphResult, GraphServiceResult,
    GraphUnixSocketResult,
};

use twin_app::{GraphEvidenceLine, RuntimeDependency};

use crate::output::format::{Lines, ScanFreshness, Status};

pub fn render(result: &GraphResult) -> String {
    render_with_scan(result, None)
}

pub fn render_with_scan(result: &GraphResult, scan: Option<ScanFreshness>) -> String {
    match result {
        GraphResult::List(list) => render_list(list, scan),
        GraphResult::Node(node) => render_node(node, scan),
        GraphResult::Service(service) => render_service(service, scan),
        GraphResult::Port(port) => render_port(port, scan),
        GraphResult::UnixSocket(unix) => render_unix_socket(unix, scan),
        GraphResult::File(file) => render_file(file, scan),
        GraphResult::Mount(mount) => render_mount(mount, scan),
        GraphResult::Directory(directory) => render_directory(directory, scan),
    }
}

fn graph_header(view: &str, scan: Option<ScanFreshness>) -> Lines {
    let mut out = Lines::new();
    out.title("twin graph");
    if let Some(scan) = scan {
        out.status_row(
            Status::Neutral,
            "scan",
            &format!("{} (fresh)", scan.duration_label()),
        );
    }
    out.status_row(Status::Ok, "view", view);
    out.blank();
    out
}

fn render_list(list: &GraphListResult, scan: Option<ScanFreshness>) -> String {
    let title = match list.kind {
        twin_core::NodeKind::Process => "Process tree",
        twin_core::NodeKind::Service => "Service list",
        twin_core::NodeKind::Port => "Port list",
        other => return format!("Unsupported list kind: {other}"),
    };
    let out = graph_header(&format!("{title}, {} nodes", list.nodes.len()), scan);
    let mut lines = Vec::new();

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

    if list.kind == twin_core::NodeKind::Service || list.kind == twin_core::NodeKind::Port {
        let mut nodes = list.nodes.clone();
        nodes.sort_by_key(|n| n.id.clone());
        for (i, node) in nodes.iter().enumerate() {
            let is_last = i + 1 == nodes.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(&node.id, &node.label)));
        }
    } else if roots.is_empty() {
        for node in &list.nodes {
            render_tree_branch(
                &mut lines,
                &node.id,
                &labels[&node.id],
                &children,
                &labels,
                "",
                true,
            );
        }
    } else {
        for (i, root) in roots.iter().enumerate() {
            let is_last_root = i + 1 == roots.len();
            let label = labels.get(root).map(String::as_str).unwrap_or("?");
            render_tree_branch(
                &mut lines,
                root,
                label,
                &children,
                &labels,
                "",
                is_last_root,
            );
            if !is_last_root {
                lines.push(String::new());
            }
        }
    }

    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_service(service: &GraphServiceResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("service neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!("{}  {}", service.service.id, service.service.label));
    lines.push(String::new());
    lines.push("owns (inferred)".to_string());
    if service.owned_processes.is_empty() && service.owned_cgroups.is_empty() {
        lines.push("└── (no owned processes or cgroups)".to_string());
    } else {
        let mut owned: Vec<(&str, &str)> = Vec::new();
        for node in &service.owned_processes {
            owned.push((&node.id, &node.label));
        }
        for node in &service.owned_cgroups {
            owned.push((&node.id, &node.label));
        }
        for (i, (id, label)) in owned.iter().enumerate() {
            let is_last = i + 1 == owned.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(id, label)));
        }
    }
    if !service.listening_ports.is_empty() {
        lines.push(String::new());
        lines.push("listens on (inferred)".to_string());
        for (i, port) in service.listening_ports.iter().enumerate() {
            let is_last = i + 1 == service.listening_ports.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(&port.id, &port.label)));
        }
    }
    if !service.listening_unix.is_empty() {
        lines.push(String::new());
        lines.push("listens on unix (inferred)".to_string());
        for (i, sock) in service.listening_unix.iter().enumerate() {
            let is_last = i + 1 == service.listening_unix.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(&sock.id, &sock.label)));
        }
    }
    if !service.connected_ports.is_empty() {
        lines.push(String::new());
        lines.push("connects to (inferred from active TCP)".to_string());
        for (i, port) in service.connected_ports.iter().enumerate() {
            let is_last = i + 1 == service.connected_ports.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(&port.id, &port.label)));
        }
    }
    if !service.connected_unix.is_empty() {
        lines.push(String::new());
        lines.push("connects to unix (inferred)".to_string());
        for (i, sock) in service.connected_unix.iter().enumerate() {
            let is_last = i + 1 == service.connected_unix.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}", format_peer(&sock.id, &sock.label)));
        }
    }
    render_dependency_section(
        &mut lines,
        "depends on (declared)",
        service
            .dependencies
            .iter()
            .filter(|d| d.tag.as_deref() == Some("declared")),
    );
    render_dependency_section(
        &mut lines,
        "depends on (runtime inferred)",
        service
            .dependencies
            .iter()
            .filter(|d| d.tag.as_deref() != Some("declared") && d.tag.as_deref() != Some("proxy")),
    );
    render_dependency_section(
        &mut lines,
        "proxies to",
        service
            .dependencies
            .iter()
            .filter(|d| d.tag.as_deref() == Some("proxy")),
    );
    render_dependency_section(
        &mut lines,
        "socket activation",
        service.socket_activation.iter(),
    );
    render_dependency_section(
        &mut lines,
        "depended on by (runtime)",
        service.dependents.iter(),
    );
    render_dependency_section(
        &mut lines,
        "depended on by (configured inactive)",
        service.configured_dependents.iter(),
    );
    render_dependency_section(&mut lines, "configured by", service.configured_files.iter());
    append_runtime_dependencies(&mut lines, &service.runtime_dependencies);
    append_evidence_lines(&mut lines, &service.evidence);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_dependency_section<'a>(
    lines: &mut Vec<String>,
    title: &str,
    deps: impl Iterator<Item = &'a GraphOwnedNode>,
) {
    let deps: Vec<_> = deps.collect();
    if deps.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push(title.to_string());
    for (i, dep) in deps.iter().enumerate() {
        let is_last = i + 1 == deps.len();
        let branch = if is_last { "└──" } else { "├──" };
        lines.push(format!("{branch} {}", format_peer(&dep.id, &dep.label)));
    }
}

fn render_file(file: &GraphFileResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("file neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!("{}  {}", file.file.id, file.file.label));
    lines.push(String::new());
    render_dependency_section(&mut lines, "configures", file.configures.iter());
    render_dependency_section(&mut lines, "references", file.references.iter());
    append_evidence_lines(&mut lines, &file.evidence);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_mount(mount: &GraphMountResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("mount neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!("{}  {}", mount.mount.id, mount.mount.label));
    let usage = match mount.used_percent {
        Some(percent) => format!("{percent}% used"),
        None => "usage unknown".to_string(),
    };
    lines.push(usage);
    render_dependency_section(&mut lines, "mounted", mount.mounted.iter());
    render_affected(&mut lines, &mount.affected);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_directory(directory: &GraphDirectoryResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("directory neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!(
        "{}  {}",
        directory.directory.id, directory.directory.label
    ));
    if let Some(mount) = &directory.mount {
        lines.push(String::new());
        lines.push("mounted on".to_string());
        lines.push(format!("└── {}", format_peer(&mount.id, &mount.label)));
    }
    render_affected(&mut lines, &directory.services);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_affected(lines: &mut Vec<String>, nodes: &[GraphOwnedNode]) {
    if nodes.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("likely affected".to_string());
    for (i, node) in nodes.iter().enumerate() {
        let is_last = i + 1 == nodes.len();
        let branch = if is_last { "└──" } else { "├──" };
        let detail = node.tag.as_deref().unwrap_or(node.label.as_str());
        lines.push(format!("{branch} {}  {detail}", node.label));
    }
}

fn render_port(port: &GraphPortResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("port listener neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!("{}  {}", port.port.id, port.port.label));
    lines.push(String::new());
    lines.push("listeners".to_string());
    let mut listeners: Vec<(&str, &str, &str)> = Vec::new();
    for node in &port.process_listeners {
        listeners.push((&node.id, &node.label, "observed"));
    }
    for node in &port.service_listeners {
        listeners.push((&node.id, &node.label, "inferred"));
    }
    if listeners.is_empty() {
        lines.push("└── (no listeners in graph)".to_string());
    } else {
        for (i, (id, label, class)) in listeners.iter().enumerate() {
            let is_last = i + 1 == listeners.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}  {class}", format_peer(id, label)));
        }
    }
    let mut callers: Vec<(&str, &str, &str)> = Vec::new();
    for node in &port.process_callers {
        callers.push((&node.id, &node.label, "observed"));
    }
    for node in &port.service_callers {
        callers.push((&node.id, &node.label, "inferred"));
    }
    if !callers.is_empty() {
        lines.push(String::new());
        lines.push("callers".to_string());
        for (i, (id, label, class)) in callers.iter().enumerate() {
            let is_last = i + 1 == callers.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}  {class}", format_peer(id, label)));
        }
    }
    append_evidence_lines(&mut lines, &port.evidence);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn render_unix_socket(unix: &GraphUnixSocketResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("unix socket neighborhood", scan);
    let mut lines = Vec::new();
    lines.push(format!(
        "{}  {}",
        unix.unix_socket.id, unix.unix_socket.label
    ));
    lines.push(String::new());
    lines.push("listeners".to_string());
    let mut listeners: Vec<(&str, &str, &str)> = Vec::new();
    for node in &unix.process_listeners {
        listeners.push((&node.id, &node.label, "observed"));
    }
    for node in &unix.service_listeners {
        listeners.push((&node.id, &node.label, "inferred"));
    }
    if listeners.is_empty() {
        lines.push("└── (no listeners in graph)".to_string());
    } else {
        for (i, (id, label, class)) in listeners.iter().enumerate() {
            let is_last = i + 1 == listeners.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}  {class}", format_peer(id, label)));
        }
    }
    let mut callers: Vec<(&str, &str, &str)> = Vec::new();
    for node in &unix.process_callers {
        callers.push((&node.id, &node.label, "observed"));
    }
    for node in &unix.service_callers {
        callers.push((&node.id, &node.label, "inferred"));
    }
    if !callers.is_empty() {
        lines.push(String::new());
        lines.push("callers".to_string());
        for (i, (id, label, class)) in callers.iter().enumerate() {
            let is_last = i + 1 == callers.len();
            let branch = if is_last { "└──" } else { "├──" };
            lines.push(format!("{branch} {}  {class}", format_peer(id, label)));
        }
    }
    append_evidence_lines(&mut lines, &unix.evidence);
    format!("{}\n{}", out.into_string(), lines.join("\n"))
}

fn append_runtime_dependencies(lines: &mut Vec<String>, dependencies: &[RuntimeDependency]) {
    if dependencies.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("Runtime dependency observed:".to_string());
    for dependency in dependencies {
        lines.push(String::new());
        lines.push(format!(
            "{} CONNECTS_TO {}",
            dependency.from_id, dependency.to_id
        ));
        lines.push(String::new());
        lines.push(format!(
            "Evidence strength: {}",
            strength_words(&dependency.evidence_label)
        ));
        lines.push("Reason:".to_string());
        for reason in &dependency.reasons {
            lines.push(format!("- {reason}"));
        }
    }
}

fn strength_words(label: &str) -> &str {
    match label {
        "very_strong" => "very strong",
        other => other,
    }
}

fn append_evidence_lines(lines: &mut Vec<String>, evidence: &[GraphEvidenceLine]) {
    let deduped = dedupe_graph_evidence(evidence);
    if deduped.is_empty() {
        return;
    }
    lines.push(String::new());
    lines.push("evidence".to_string());
    for (i, line) in deduped.iter().enumerate() {
        let is_last = i + 1 == deduped.len();
        let prefix = if is_last { "└──" } else { "├──" };
        lines.push(format!("{prefix} {} {}", line.source, line.statement));
        lines.push(format!("    relationship: {}", line.relationship));
    }
}

fn dedupe_graph_evidence(evidence: &[GraphEvidenceLine]) -> Vec<&GraphEvidenceLine> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for line in evidence {
        let key = graph_evidence_key(line);
        if seen.insert(key) {
            out.push(line);
        }
    }
    out
}

fn graph_evidence_key(line: &GraphEvidenceLine) -> String {
    format!(
        "{}|{}|{}",
        line.relationship,
        normalize_evidence_source(&line.source),
        normalize_evidence_statement(&line.statement)
    )
}

fn normalize_evidence_source(source: &str) -> String {
    if let Some((head, _)) = source.rsplit_once(':') {
        if head.contains("/proc/net/") {
            return format!("{head}:*");
        }
    }
    source.to_string()
}

fn normalize_evidence_statement(statement: &str) -> String {
    if let Some(inode_pos) = statement.find("inode ") {
        let mut core = statement[inode_pos..].to_string();
        if let Some(joined) = core.find(" joined with ") {
            core.truncate(joined);
        }
        return core;
    }
    statement.to_string()
}

fn render_node(node: &GraphNodeResult, scan: Option<ScanFreshness>) -> String {
    let out = graph_header("process neighborhood", scan);
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

    format!("{}\n{}", out.into_string(), lines.join("\n"))
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
    lines.push(format!("{prefix}{branch} {}", format_peer(node_id, label)));
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
    std::str::FromStr::from_str(node_id)
        .ok()
        .and_then(|id: twin_core::NodeId| id.process_pid())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_duplicate_ppid_evidence() {
        let refs: Vec<String> = (2..=5).map(|n| format!("/proc/{n}/stat ppid=1")).collect();
        let summary = summarize_child_stat_evidence(&refs, 4).expect("summary");
        assert!(summary.contains("4 children"));
        assert!(summary.contains("ppid=1"));
    }

    #[test]
    fn tree_branch_renders_connector() {
        let mut lines = Vec::new();
        let children = BTreeMap::new();
        let labels = HashMap::from([("process:pid:1".to_string(), "systemd".to_string())]);
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
