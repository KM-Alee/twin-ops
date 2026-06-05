use std::str::FromStr;

use twin_app::ImpactRequest;
use twin_core::NodeId;

use super::args::ImpactArgs;

pub fn impact_request(args: &ImpactArgs) -> ImpactRequest {
    if let Ok(id) = NodeId::from_str(&args.target) {
        return ImpactRequest {
            config_override: args.config.clone(),
            target: Some(id),
            target_query: None,
        };
    }
    ImpactRequest {
        config_override: args.config.clone(),
        target: None,
        target_query: Some(args.target.clone()),
    }
}
