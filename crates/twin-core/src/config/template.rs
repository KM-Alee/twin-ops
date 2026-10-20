pub const DEFAULT_CONFIG_TOML: &str = r#"# twin configuration

[retention]
raw_observations_days = 7
graph_history_days = 30
ebpf_events_days = 3

# Closed connections still count as runtime dependencies for this many days after the
# last sighting (idle pools, periodic jobs).
[dependencies]
window_days = 7
"#;
