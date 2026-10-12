use crate::{AppError, GraphError};
use twin_core::{NodeId, NodeKind, NodeState};
use twin_store::Store;

pub fn parse_port_query(query: &str) -> Result<Option<u16>, AppError> {
    let Some(port) = query.strip_prefix(':') else {
        return Ok(None);
    };
    port.parse::<u16>()
        .ok()
        .filter(|port| *port > 0)
        .map(Some)
        .ok_or_else(|| AppError::InvalidPortQuery {
            query: query.into(),
        })
}

pub(crate) fn resolve_port_target(store: &Store, port: u16) -> Result<NodeId, AppError> {
    let suffix = format!(":{port}");
    let mut matches: Vec<_> = store
        .list_nodes_by_kind_typed(NodeKind::Port)
        .map_err(GraphError::Store)?
        .into_iter()
        .filter(|node| {
            node.state() == NodeState::Active
                && node.id().as_str().starts_with("port:tcp:")
                && node.id().as_str().ends_with(&suffix)
        })
        .map(|node| node.id().clone())
        .collect();
    matches.sort_by(|a, b| a.as_str().cmp(b.as_str()));
    match matches.as_slice() {
        [node] => Ok(node.clone()),
        [] => Err(AppError::PortQueryNotFound { port }),
        _ => Err(AppError::AmbiguousPortQuery {
            port,
            candidates: matches
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join(", "),
        }),
    }
}
