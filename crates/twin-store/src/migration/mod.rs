pub const MIGRATION_001: &str = r"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at_ns INTEGER NOT NULL
);
";

pub const MIGRATION_002: &str = r"
CREATE TABLE IF NOT EXISTS collector_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    collector TEXT NOT NULL,
    started_at_ns INTEGER NOT NULL,
    ended_at_ns INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'success',
    observation_count INTEGER NOT NULL DEFAULT 0,
    warning_count INTEGER NOT NULL DEFAULT 0,
    error_message TEXT
);

CREATE INDEX IF NOT EXISTS idx_collector_runs_collector ON collector_runs(collector);
CREATE INDEX IF NOT EXISTS idx_collector_runs_started ON collector_runs(started_at_ns);

CREATE TABLE IF NOT EXISTS nodes (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    label TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL,
    valid_from_ns INTEGER NOT NULL,
    valid_to_ns INTEGER,
    metadata_json TEXT NOT NULL DEFAULT '{}'
);

CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);
CREATE INDEX IF NOT EXISTS idx_nodes_state ON nodes(state);
CREATE INDEX IF NOT EXISTS idx_nodes_last_seen ON nodes(last_seen_ns);

CREATE TABLE IF NOT EXISTS observations (
    id TEXT PRIMARY KEY,
    source TEXT NOT NULL,
    kind TEXT NOT NULL,
    subject_node_id TEXT,
    object_node_id TEXT,
    timestamp_ns INTEGER NOT NULL,
    confidence_hint TEXT NOT NULL DEFAULT 'moderate',
    redaction_state TEXT NOT NULL DEFAULT 'none',
    metadata_json TEXT NOT NULL DEFAULT '{}',
    collector_run_id INTEGER,
    FOREIGN KEY (collector_run_id) REFERENCES collector_runs(id)
);

CREATE INDEX IF NOT EXISTS idx_observations_source ON observations(source);
CREATE INDEX IF NOT EXISTS idx_observations_kind ON observations(kind);
CREATE INDEX IF NOT EXISTS idx_observations_subject ON observations(subject_node_id);
CREATE INDEX IF NOT EXISTS idx_observations_timestamp ON observations(timestamp_ns);

CREATE TABLE IF NOT EXISTS edges (
    id TEXT PRIMARY KEY,
    from_node_id TEXT NOT NULL,
    to_node_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    class TEXT NOT NULL,
    state TEXT NOT NULL DEFAULT 'active',
    evidence_score INTEGER NOT NULL DEFAULT 0,
    evidence_label TEXT NOT NULL DEFAULT 'weak',
    evidence_count INTEGER NOT NULL DEFAULT 0,
    first_seen_ns INTEGER NOT NULL,
    last_seen_ns INTEGER NOT NULL,
    metadata_json TEXT NOT NULL DEFAULT '{}',
    FOREIGN KEY (from_node_id) REFERENCES nodes(id),
    FOREIGN KEY (to_node_id) REFERENCES nodes(id)
);

CREATE INDEX IF NOT EXISTS idx_edges_from ON edges(from_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_to ON edges(to_node_id);
CREATE INDEX IF NOT EXISTS idx_edges_kind ON edges(kind);
CREATE INDEX IF NOT EXISTS idx_edges_class ON edges(class);
CREATE INDEX IF NOT EXISTS idx_edges_state ON edges(state);

CREATE TABLE IF NOT EXISTS edge_observations (
    edge_id TEXT NOT NULL,
    observation_id TEXT NOT NULL,
    role TEXT NOT NULL DEFAULT 'direct',
    PRIMARY KEY (edge_id, observation_id),
    FOREIGN KEY (edge_id) REFERENCES edges(id),
    FOREIGN KEY (observation_id) REFERENCES observations(id)
);

CREATE INDEX IF NOT EXISTS idx_edge_obs_role ON edge_observations(role);
";

pub const LATEST_VERSION: i64 = 2;
