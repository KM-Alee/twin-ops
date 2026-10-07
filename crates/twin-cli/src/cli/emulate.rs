use std::str::FromStr;

use twin_app::{validate_max_depth, AppError, EmulateActionRequest, EmulateError, EmulateRequest};
use twin_core::NodeId;

use super::args::{EmulateDeleteArgs, EmulateFillDiskArgs, EmulateRestartArgs};

pub fn emulate_restart_request(args: &EmulateRestartArgs) -> Result<EmulateRequest, AppError> {
    let max_depth = validate_max_depth(args.max_depth as usize)?;
    let restart = |target: Option<NodeId>, target_query: Option<String>| EmulateRequest {
        config_override: args.config.clone(),
        show_evidence: args.evidence,
        action: EmulateActionRequest::Restart {
            target,
            target_query,
            show_paths: args.paths,
            max_depth,
        },
    };
    if args.target.starts_with("port:tcp:") || args.target.starts_with("unix:") {
        let id =
            NodeId::from_str(&args.target).map_err(|source| AppError::InvalidEmulateTarget {
                value: args.target.clone(),
                source,
            })?;
        return Ok(restart(Some(id), None));
    }
    if let Ok(id) = NodeId::from_str(&args.target) {
        return Ok(restart(Some(id), None));
    }
    Ok(restart(None, Some(args.target.clone())))
}

pub fn emulate_fill_disk_request(args: &EmulateFillDiskArgs) -> Result<EmulateRequest, AppError> {
    let to_percent = parse_fill_percent(&args.to_percent)?;
    Ok(EmulateRequest {
        config_override: args.config.clone(),
        show_evidence: args.evidence,
        action: EmulateActionRequest::FillDisk {
            mount: args.target.clone(),
            to_percent,
        },
    })
}

fn parse_fill_percent(raw: &str) -> Result<u8, AppError> {
    let trimmed = raw.trim().trim_end_matches('%').trim();
    let percent = trimmed.parse::<u8>().map_err(|_| {
        AppError::Emulate(EmulateError::InvalidFillPercent {
            value: raw.to_string(),
        })
    })?;
    if !(1..=100).contains(&percent) {
        return Err(AppError::Emulate(EmulateError::InvalidFillPercent {
            value: raw.to_string(),
        }));
    }
    Ok(percent)
}

pub fn emulate_delete_request(args: &EmulateDeleteArgs) -> Result<EmulateRequest, AppError> {
    Ok(EmulateRequest {
        config_override: args.config.clone(),
        show_evidence: args.evidence,
        action: EmulateActionRequest::DeleteFile {
            path: args.target.clone(),
        },
    })
}
