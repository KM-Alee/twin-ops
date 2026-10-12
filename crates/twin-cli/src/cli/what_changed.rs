use twin_app::WhatChangedRequest;

use crate::cli::args::WhatChangedArgs;

pub fn what_changed_request(args: &WhatChangedArgs) -> WhatChangedRequest {
    WhatChangedRequest {
        config_override: args.config.clone(),
        since: args.since.clone(),
        verbose: args.verbose,
        target_query: args.target.clone(),
        categories: args.categories.clone(),
        limit: args.limit as usize,
        max_depth: args.max_depth,
    }
}
