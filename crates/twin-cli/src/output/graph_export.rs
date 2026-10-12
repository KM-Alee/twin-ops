use std::collections::{BTreeMap, BTreeSet};

use twin_app::{GraphNodeSummary, GraphOwnedNode, GraphResult};

use crate::cli::args::GraphFormat;

#[derive(Default)]
struct ExportGraph {
    nodes: BTreeMap<String, String>,
    edges: BTreeSet<(String, String, String)>,
}

impl ExportGraph {
    fn node(&mut self, node: &GraphNodeSummary) {
        self.nodes.insert(node.id.clone(), node.label.clone());
    }

    fn owned(
        &mut self,
        root: &GraphNodeSummary,
        peers: &[GraphOwnedNode],
        kind: &str,
        incoming: bool,
    ) {
        self.node(root);
        for peer in peers {
            self.nodes.insert(peer.id.clone(), peer.label.clone());
            let (from, to) = if incoming {
                (&peer.id, &root.id)
            } else {
                (&root.id, &peer.id)
            };
            self.edges
                .insert((from.clone(), to.clone(), kind.to_string()));
        }
    }

    fn from_result(result: &GraphResult) -> Self {
        let mut graph = Self::default();
        match result {
            GraphResult::List(list) => {
                for node in &list.nodes {
                    graph.node(node);
                }
                for edge in &list.parent_edges {
                    graph.edges.insert((
                        edge.parent_id.clone(),
                        edge.child_id.clone(),
                        "parent_of".into(),
                    ));
                }
            }
            GraphResult::Node(view) => {
                graph.node(&view.node);
                for (incoming, edges) in [(true, &view.incoming), (false, &view.outgoing)] {
                    for edge in edges {
                        graph
                            .nodes
                            .insert(edge.peer_id.clone(), edge.peer_label.clone());
                        let (from, to) = if incoming {
                            (&edge.peer_id, &view.node.id)
                        } else {
                            (&view.node.id, &edge.peer_id)
                        };
                        graph
                            .edges
                            .insert((from.clone(), to.clone(), edge.kind.clone()));
                    }
                }
            }
            GraphResult::Service(view) => {
                for (peers, kind, incoming) in [
                    (&view.owned_processes, "owns", false),
                    (&view.owned_cgroups, "owns", false),
                    (&view.listening_ports, "listens_on", false),
                    (&view.listening_unix, "listens_on", false),
                    (&view.connected_ports, "connects_to", false),
                    (&view.connected_unix, "connects_to", false),
                    (&view.dependencies, "depends_on", false),
                    (&view.dependents, "depends_on", true),
                    (&view.configured_dependents, "depends_on", true),
                    (&view.configured_files, "configured_by", false),
                ] {
                    graph.owned(&view.service, peers, kind, incoming);
                }
                for dependency in &view.runtime_dependencies {
                    graph
                        .nodes
                        .entry(dependency.to_id.clone())
                        .or_insert_with(|| dependency.to_id.clone());
                    graph.edges.insert((
                        dependency.from_id.clone(),
                        dependency.to_id.clone(),
                        dependency.relationship.clone(),
                    ));
                }
            }
            GraphResult::Port(view) => {
                for (peers, kind) in [
                    (&view.process_listeners, "listens_on"),
                    (&view.service_listeners, "listens_on"),
                    (&view.process_callers, "connects_to"),
                    (&view.service_callers, "connects_to"),
                ] {
                    graph.owned(&view.port, peers, kind, true);
                }
            }
            GraphResult::UnixSocket(view) => {
                for (peers, kind) in [
                    (&view.process_listeners, "listens_on"),
                    (&view.service_listeners, "listens_on"),
                    (&view.process_callers, "connects_to"),
                    (&view.service_callers, "connects_to"),
                ] {
                    graph.owned(&view.unix_socket, peers, kind, true);
                }
            }
            GraphResult::File(view) => {
                graph.owned(&view.file, &view.configures, "configured_by", true);
                graph.owned(&view.file, &view.references, "references", false);
            }
            GraphResult::Mount(view) => {
                graph.owned(&view.mount, &view.mounted, "mounted_on", true);
            }
            GraphResult::Directory(view) => {
                graph.owned(&view.directory, &view.services, "uses", true);
                graph.owned(&view.directory, view.mount.as_slice(), "mounted_on", false);
            }
            GraphResult::Library(view) => {
                graph.owned(&view.library, &view.loaded_by, "loads_library", true);
                graph.owned(&view.library, &view.installed_by, "installed_by", false);
            }
            GraphResult::Package(view) => {
                graph.owned(&view.package, &view.libraries, "installed_by", true);
                graph.owned(&view.package, &view.services, "depends_on", true);
            }
            GraphResult::Container(view) => {
                graph.owned(&view.container, view.image.as_slice(), "runs_image", false);
                graph.owned(&view.container, &view.mounts, "mounts_volume", false);
                graph.owned(&view.container, &view.processes, "owns", false);
                for port in &view.ports {
                    graph
                        .nodes
                        .insert(port.port_id.clone(), port.port_id.clone());
                    graph.edges.insert((
                        view.container.id.clone(),
                        port.port_id.clone(),
                        "maps_port".into(),
                    ));
                }
            }
        }
        graph
    }
}

pub fn render(result: &GraphResult, format: GraphFormat) -> String {
    let graph = ExportGraph::from_result(result);
    let ids: BTreeMap<&str, usize> = graph
        .nodes
        .keys()
        .enumerate()
        .map(|(index, id)| (id.as_str(), index))
        .collect();
    let mut lines = vec![match format {
        GraphFormat::Dot => "digraph twin {",
        _ => "flowchart LR",
    }
    .to_string()];
    for (id, label) in &graph.nodes {
        let index = ids[id.as_str()];
        let label = format!("{id}\n{label}");
        lines.push(match format {
            GraphFormat::Dot => format!("  n{index} [label=\"{}\"];", dot_escape(&label)),
            _ => format!("  n{index}[\"{}\"]", mermaid_escape(&label)),
        });
    }
    for (from, to, kind) in &graph.edges {
        let (Some(from), Some(to)) = (ids.get(from.as_str()), ids.get(to.as_str())) else {
            continue;
        };
        lines.push(match format {
            GraphFormat::Dot => format!("  n{from} -> n{to} [label=\"{}\"];", dot_escape(kind)),
            _ => format!("  n{from} -->|{}| n{to}", mermaid_escape(kind)),
        });
    }
    if format == GraphFormat::Dot {
        lines.push("}".into());
    }
    lines.join("\n")
}

fn dot_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
}

fn mermaid_escape(text: &str) -> String {
    text.replace('&', "#38;")
        .replace('"', "#34;")
        .replace('<', "#60;")
        .replace('>', "#62;")
        .replace('[', "#91;")
        .replace(']', "#93;")
        .replace('|', "#124;")
        .replace('\n', "<br/>")
        .replace('\r', "")
}
