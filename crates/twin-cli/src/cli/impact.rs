use std::str::FromStr;

use twin_app::{AppError, ImpactRequest};
use twin_core::NodeId;

use super::args::ImpactArgs;

pub fn impact_request(args: &ImpactArgs) -> Result<ImpactRequest, AppError> {
    if args.target.starts_with("port:tcp:") || args.target.starts_with("unix:") {
        let id =
            NodeId::from_str(&args.target).map_err(|source| AppError::InvalidImpactTarget {
                value: args.target.clone(),
                source,
            })?;
        return Ok(ImpactRequest {
            config_override: args.config.clone(),
            target: Some(id),
            target_query: None,
        });
    }
    if let Ok(id) = NodeId::from_str(&args.target) {
        return Ok(ImpactRequest {
            config_override: args.config.clone(),
            target: Some(id),
            target_query: None,
        });
    }
    Ok(ImpactRequest {
        config_override: args.config.clone(),
        target: None,
        target_query: Some(args.target.clone()),
    })
}
