use std::str::FromStr;

use twin_app::{AppError, EmulateRequest};
use twin_core::NodeId;

use super::args::EmulateRestartArgs;

pub fn emulate_restart_request(args: &EmulateRestartArgs) -> Result<EmulateRequest, AppError> {
    if args.target.starts_with("port:tcp:") || args.target.starts_with("unix:") {
        let id =
            NodeId::from_str(&args.target).map_err(|source| AppError::InvalidEmulateTarget {
                value: args.target.clone(),
                source,
            })?;
        return Ok(EmulateRequest {
            config_override: args.config.clone(),
            target: Some(id),
            target_query: None,
        });
    }
    if let Ok(id) = NodeId::from_str(&args.target) {
        return Ok(EmulateRequest {
            config_override: args.config.clone(),
            target: Some(id),
            target_query: None,
        });
    }
    Ok(EmulateRequest {
        config_override: args.config.clone(),
        target: None,
        target_query: Some(args.target.clone()),
    })
}
