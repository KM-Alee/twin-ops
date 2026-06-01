use std::str::FromStr;

use twin_app::{AppError, GraphRequest};
use twin_core::{NodeId, NodeKind, ParseError};

use super::args::GraphArgs;

enum GraphQuery {
    List(NodeKind),
    Node(NodeId),
}

pub fn graph_request(args: &GraphArgs) -> Result<GraphRequest, AppError> {
    let query = if let Some(value) = &args.target {
        parse_positional(value)?
    } else if let Some(value) = &args.kind {
        GraphQuery::List(parse_kind(value)?)
    } else {
        GraphQuery::List(NodeKind::Process)
    };

    let (kind, target) = match query {
        GraphQuery::List(kind) => (Some(kind), None),
        GraphQuery::Node(id) => (None, Some(id)),
    };

    Ok(GraphRequest {
        config_override: args.config.clone(),
        kind,
        target,
    })
}

fn parse_positional(value: &str) -> Result<GraphQuery, AppError> {
    if let Ok(id) = NodeId::from_str(value) {
        return Ok(GraphQuery::Node(id));
    }
    if let Some(pid) = parse_pid_shorthand(value) {
        return Ok(GraphQuery::Node(NodeId::process(pid)));
    }
    if let Ok(kind) = NodeKind::from_str(value) {
        return Ok(GraphQuery::List(kind));
    }
    Err(invalid_graph_target(value))
}

fn parse_kind(value: &str) -> Result<NodeKind, AppError> {
    NodeKind::from_str(value).map_err(|source| AppError::InvalidGraphTarget {
        value: value.to_string(),
        source,
    })
}

fn parse_pid_shorthand(value: &str) -> Option<u32> {
    if let Some(pid) = value.strip_prefix("pid:") {
        return pid.parse().ok();
    }
    if !value.is_empty() && value.chars().all(|c| c.is_ascii_digit()) {
        return value.parse().ok();
    }
    None
}

fn invalid_graph_target(value: &str) -> AppError {
    AppError::InvalidGraphTarget {
        value: value.to_string(),
        source: ParseError::InvalidNodeId {
            value: value.to_string(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(args: GraphArgs) -> GraphRequest {
        graph_request(&args).expect("graph request")
    }

    #[test]
    fn defaults_to_process_list() {
        let req = request(GraphArgs {
            config: None,
            kind: None,
            target: None,
        });
        assert_eq!(req.kind, Some(NodeKind::Process));
        assert!(req.target.is_none());
    }

    #[test]
    fn positional_process_is_kind_list() {
        let req = request(GraphArgs {
            config: None,
            kind: None,
            target: Some("process".to_string()),
        });
        assert_eq!(req.kind, Some(NodeKind::Process));
        assert!(req.target.is_none());
    }

    #[test]
    fn positional_pid_shorthands() {
        for value in ["1234", "pid:1234", "process:pid:1234"] {
            let req = request(GraphArgs {
                config: None,
                kind: None,
                target: Some(value.to_string()),
            });
            assert_eq!(req.target, Some(NodeId::process(1234)));
            assert!(req.kind.is_none());
        }
    }

    #[test]
    fn kind_flag_still_works() {
        let req = request(GraphArgs {
            config: None,
            kind: Some("process".to_string()),
            target: None,
        });
        assert_eq!(req.kind, Some(NodeKind::Process));
    }

    #[test]
    fn unknown_target_is_rejected() {
        let err = graph_request(&GraphArgs {
            config: None,
            kind: None,
            target: Some("nginx".to_string()),
        })
        .expect_err("invalid target");
        assert!(matches!(err, AppError::InvalidGraphTarget { .. }));
    }
}
