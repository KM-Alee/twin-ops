use twin_app::{SnapshotCreateResult, SnapshotListResult};

use crate::output::format::{format_age, Lines, Status};

pub fn render_create(result: &SnapshotCreateResult) -> String {
    let mut out = Lines::new();
    out.title("twin snapshot create");
    out.status_row(Status::Ok, "name", &result.name);
    out.tree_leaf(false, "nodes", &result.node_count.to_string());
    out.tree_leaf(true, "edges", &result.edge_count.to_string());
    out.into_string()
}

pub fn render_list(result: &SnapshotListResult) -> String {
    let mut out = Lines::new();
    out.title("twin snapshot list");
    if result.snapshots.is_empty() {
        out.status_row(Status::Neutral, "snapshots", "none");
        return out.into_string();
    }
    out.status_row(
        Status::Ok,
        "snapshots",
        &format!("{} checkpoint(s)", result.snapshots.len()),
    );
    out.blank();
    for (i, snap) in result.snapshots.iter().enumerate() {
        let is_last = i + 1 == result.snapshots.len();
        let age = format_age(snap.created_at_ns);
        out.tree_leaf(
            is_last,
            &snap.name,
            &format!(
                "{age} · {} nodes · {} edges",
                snap.node_count, snap.edge_count
            ),
        );
    }
    out.into_string()
}
