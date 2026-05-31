use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TwinConfig {
    pub retention: RetentionConfig,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RetentionConfig {
    #[serde(default = "default_observations_days")]
    pub raw_observations_days: u32,
    #[serde(default = "default_graph_days")]
    pub graph_history_days: u32,
    #[serde(default = "default_ebpf_days")]
    pub ebpf_events_days: u32,
}

impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            raw_observations_days: default_observations_days(),
            graph_history_days: default_graph_days(),
            ebpf_events_days: default_ebpf_days(),
        }
    }
}

fn default_observations_days() -> u32 {
    7
}

fn default_graph_days() -> u32 {
    30
}

fn default_ebpf_days() -> u32 {
    3
}
