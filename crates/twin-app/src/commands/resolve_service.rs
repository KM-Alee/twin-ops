use std::str::FromStr;

use twin_core::{NodeId, NodeKind};
use twin_store::Store;

use crate::error::{AppError, GraphError, ImpactError};

#[derive(Debug, Clone)]
pub enum ServiceNotFoundContext {
    Graph { detail: String },
    Impact,
}

pub fn resolve_service_target(
    store: &Store,
    query: &str,
    not_found: ServiceNotFoundContext,
) -> Result<NodeId, AppError> {
    if let Ok(id) = NodeId::from_str(query) {
        if store
            .get_node_typed(&id)
            .map_err(store_err(&not_found))?
            .is_some()
        {
            return Ok(id);
        }
        if id.kind() == Some(NodeKind::Service) {
            return Err(node_not_found(id, &not_found));
        }
    }

    let unit = if query.ends_with(".service") {
        query.to_string()
    } else {
        format!("{query}.service")
    };
    let exact = NodeId::service(&unit);
    if store
        .get_node_typed(&exact)
        .map_err(store_err(&not_found))?
        .is_some()
    {
        return Ok(exact);
    }

    let services = store
        .list_nodes_by_kind_typed(NodeKind::Service)
        .map_err(store_err(&not_found))?;
    let needle = query.to_ascii_lowercase();
    let matches: Vec<NodeId> = services
        .iter()
        .filter(|node| {
            let id = node.id().as_str().to_ascii_lowercase();
            let label = node.label().to_ascii_lowercase();
            id.contains(&needle) || label.contains(&needle)
        })
        .map(|n| n.id().clone())
        .collect();

    match matches.len() {
        0 => Err(service_not_found(query, not_found)),
        1 => Ok(matches[0].clone()),
        _ => {
            let candidates: Vec<String> = matches.iter().map(|id| id.to_string()).collect();
            Err(ambiguous_service(query, candidates.join(", "), not_found))
        }
    }
}

fn store_err(ctx: &ServiceNotFoundContext) -> impl FnOnce(twin_store::StoreError) -> AppError + '_ {
    move |e| match ctx {
        ServiceNotFoundContext::Graph { .. } => GraphError::Store(e).into(),
        ServiceNotFoundContext::Impact => ImpactError::Store(e).into(),
    }
}

fn node_not_found(id: NodeId, ctx: &ServiceNotFoundContext) -> AppError {
    match ctx {
        ServiceNotFoundContext::Graph { .. } => GraphError::NodeNotFound { id }.into(),
        ServiceNotFoundContext::Impact => ImpactError::NodeNotFound { id }.into(),
    }
}

fn service_not_found(query: &str, ctx: ServiceNotFoundContext) -> AppError {
    match ctx {
        ServiceNotFoundContext::Graph { detail } => GraphError::ServiceNotFound {
            query: query.to_string(),
            detail,
        }
        .into(),
        ServiceNotFoundContext::Impact => ImpactError::ServiceNotFound {
            query: query.to_string(),
        }
        .into(),
    }
}

fn ambiguous_service(query: &str, candidates: String, ctx: ServiceNotFoundContext) -> AppError {
    match ctx {
        ServiceNotFoundContext::Graph { .. } => GraphError::AmbiguousService {
            query: query.to_string(),
            candidates,
        }
        .into(),
        ServiceNotFoundContext::Impact => ImpactError::AmbiguousService {
            query: query.to_string(),
            candidates,
        }
        .into(),
    }
}
