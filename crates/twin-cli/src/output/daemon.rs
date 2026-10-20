use crate::daemon::DaemonStatus;
use crate::output::format::{Lines, Status};

pub fn render_status(status: &DaemonStatus) -> String {
    let mut out = Lines::new();
    out.title("twin daemon status");
    if status.is_reachable {
        let age = status
            .graph_age_secs
            .map(|age| format!("graph {age} s old"))
            .unwrap_or_else(|| "graph age unknown".to_string());
        out.status_row(Status::Ok, "daemon", &format!("answering · {age}"));
    } else {
        out.status_row(Status::Warn, "daemon", "not available");
    }
    out.blank();
    out.section("socket");
    out.tree_leaf(
        status.version.is_none() && status.reason.is_none(),
        "path",
        &status.socket,
    );
    if let Some(version) = &status.version {
        out.tree_leaf(status.reason.is_none(), "version", version);
    }
    if let Some(reason) = &status.reason {
        out.tree_leaf(true, "reason", reason);
        out.blank();
        out.section("next");
        out.tree_leaf(
            true,
            "fallback",
            "brief, preflight and mcp answer from a local scan with your own permissions",
        );
    }
    out.into_string()
}

/// One line per daemon event, for the journal.
pub fn render_started(socket: &std::path::Path, interval: &str) -> String {
    format!(
        "twin daemon serving {} · scanning every {interval}",
        socket.display()
    )
}
