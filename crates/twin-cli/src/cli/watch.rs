use twin_app::WatchRequest;

use crate::cli::args::WatchArgs;

pub fn watch_request(args: &WatchArgs) -> WatchRequest {
    WatchRequest {
        config_override: args.config.clone(),
        interval: args.interval.clone(),
        duration: args.duration.clone(),
        max_ticks: args.ticks,
    }
}
