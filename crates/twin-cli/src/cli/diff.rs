use twin_app::DiffRequest;

use crate::cli::args::DiffArgs;

pub fn diff_request(args: &DiffArgs) -> DiffRequest {
    DiffRequest {
        config_override: args.config.clone(),
        left: args.left.clone(),
        right: args.right.clone(),
    }
}
