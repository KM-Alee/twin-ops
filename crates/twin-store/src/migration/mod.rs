pub const MIGRATION_001: &str = r"
CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at_ns INTEGER NOT NULL
);
";

pub const LATEST_VERSION: i64 = 1;
