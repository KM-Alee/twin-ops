use std::str::FromStr;

use twin_app::{validate_max_depth, AppError, ImpactRequest};
use twin_core::NodeId;

use super::args::ImpactArgs;

pub fn impact_request(args: &ImpactArgs) -> Result<ImpactRequest, AppError> {
    let max_depth = validate_max_depth(args.max_depth as usize)?;
    let common = |target: Option<NodeId>, target_query: Option<String>| ImpactRequest {
        config_override: args.config.clone(),
        target,
        target_query,
        show_paths: args.paths,
        max_depth,
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
        return Ok(common(Some(id), None));
    }
    Ok(common(None, Some(args.target.clone())))
}
