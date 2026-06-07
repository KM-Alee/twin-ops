use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotCreateResult {
    pub name: String,
    pub created_at_ns: i64,
    pub node_count: i64,
    pub edge_count: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotListResult {
    pub snapshots: Vec<SnapshotEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SnapshotEntry {
    pub name: String,
    pub created_at_ns: i64,
    pub node_count: i64,
    pub edge_count: i64,
}
