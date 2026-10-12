use crate::GraphFacts;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FactSource {
    Host,
    Tcp,
    Udp,
    Unix,
    Systemd,
    Docker,
    Containerd,
    Kubernetes,
}

pub(crate) fn has_node_gaps(facts: &GraphFacts, id: &str) -> bool {
    facts.unknowns.iter().any(|unknown| {
        unknown
            .source
            .is_none_or(|source| source_matches(source, id))
    })
}

pub(crate) fn has_network_gaps(facts: &GraphFacts, from: Option<&str>) -> bool {
    facts.unknowns.iter().any(|unknown| match unknown.source {
        None | Some(FactSource::Host | FactSource::Tcp | FactSource::Udp) => true,
        Some(source @ (FactSource::Docker | FactSource::Containerd)) => {
            from.is_some_and(|from| source_matches(source, from))
        }
        _ => false,
    })
}

fn source_matches(source: FactSource, id: &str) -> bool {
    match source {
        FactSource::Host => !id.starts_with("k8s:") && !id.starts_with("container:"),
        FactSource::Tcp => {
            id.starts_with("port:tcp:") || id.starts_with("service:") || id.starts_with("process:")
        }
        FactSource::Udp => {
            id.starts_with("port:udp:") || id.starts_with("service:") || id.starts_with("process:")
        }
        FactSource::Unix => {
            id.starts_with("unix:") || id.starts_with("service:") || id.starts_with("process:")
        }
        FactSource::Systemd => id.starts_with("service:"),
        FactSource::Docker => {
            (id.starts_with("container:") || id.starts_with("image:"))
                && !id.contains(":containerd:")
        }
        FactSource::Containerd => {
            id.starts_with("container:containerd:") || id.starts_with("image:containerd:")
        }
        FactSource::Kubernetes => id.starts_with("k8s:"),
    }
}
