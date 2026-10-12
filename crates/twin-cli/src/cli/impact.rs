use std::str::FromStr;

use twin_app::{validate_max_depth, AppError, ImpactRequest};
use twin_core::{NodeId, NodeKind};

use super::args::ImpactArgs;

pub fn impact_request(args: &ImpactArgs) -> Result<ImpactRequest, AppError> {
    twin_app::parse_port_query(&args.target)?;
    let max_depth = validate_max_depth(args.max_depth as usize)?;
    let common = |target: Option<NodeId>, target_query: Option<String>| ImpactRequest {
        config_override: args.config.clone(),
        target,
        target_query,
        show_paths: args.paths,
        max_depth,
        show_evidence: args.evidence,
    };
    if args.target.starts_with("port:tcp:") || args.target.starts_with("unix:") {
        let id =
            NodeId::from_str(&args.target).map_err(|source| AppError::InvalidImpactTarget {
                value: args.target.clone(),
                source,
            })?;
        return Ok(common(Some(id), None));
    }
    if let Ok(id) = NodeId::from_str(&args.target) {
        if !matches!(
            id.kind(),
            Some(NodeKind::Service | NodeKind::Port | NodeKind::UnixSocket | NodeKind::Package)
        ) {
            return Err(twin_app::ImpactError::UnsupportedTarget {
                kind: id.kind().map(|kind| kind.to_string()).unwrap_or_default(),
            }
            .into());
        }
        return Ok(common(Some(id), None));
    }
    Ok(common(None, Some(args.target.clone())))
}
