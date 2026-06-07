use twin_app::{SnapshotCreateRequest, SnapshotListRequest};

use crate::cli::args::{SnapshotActionArgs, SnapshotArgs};

pub fn snapshot_create_request(args: &SnapshotArgs, name: &str) -> SnapshotCreateRequest {
    SnapshotCreateRequest {
        config_override: args.config.clone(),
        name: name.to_string(),
    }
}

pub fn snapshot_list_request(args: &SnapshotArgs) -> SnapshotListRequest {
    SnapshotListRequest {
        config_override: args.config.clone(),
    }
}

pub fn snapshot_action_name(action: &SnapshotActionArgs) -> Option<&str> {
    match action {
        SnapshotActionArgs::Create { name } => Some(name.as_str()),
        SnapshotActionArgs::List => None,
    }
}
