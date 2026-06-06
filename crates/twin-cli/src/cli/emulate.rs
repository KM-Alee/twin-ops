use std::str::FromStr;

use twin_app::{AppError, EmulateActionRequest, EmulateRequest};
use twin_core::NodeId;

use super::args::{EmulateDeleteArgs, EmulateRestartArgs};

pub fn emulate_restart_request(args: &EmulateRestartArgs) -> Result<EmulateRequest, AppError> {
    if args.target.starts_with("port:tcp:") || args.target.starts_with("unix:") {
        let id =
            NodeId::from_str(&args.target).map_err(|source| AppError::InvalidEmulateTarget {
                value: args.target.clone(),
                source,
            })?;
        return Ok(EmulateRequest {
            config_override: args.config.clone(),
            action: EmulateActionRequest::Restart {
                target: Some(id),
                target_query: None,
            },
        });
    }
    if let Ok(id) = NodeId::from_str(&args.target) {
        return Ok(EmulateRequest {
            config_override: args.config.clone(),
            action: EmulateActionRequest::Restart {
                target: Some(id),
                target_query: None,
            },
        });
    }
    Ok(EmulateRequest {
        config_override: args.config.clone(),
        action: EmulateActionRequest::Restart {
            target: None,
            target_query: Some(args.target.clone()),
        },
    })
}

pub fn emulate_delete_request(args: &EmulateDeleteArgs) -> Result<EmulateRequest, AppError> {
    Ok(EmulateRequest {
        config_override: args.config.clone(),
        action: EmulateActionRequest::DeleteFile {
            path: args.target.clone(),
        },
    })
}
