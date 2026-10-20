use serde::{Deserialize, Serialize};

/// Inferred runtime dependencies stay relevant for `window_days` after their last sighting,
/// so idle connection pools and periodic clients still count as dependents.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DependencyConfig {
    #[serde(default = "default_window_days")]
    pub window_days: u32,
}

impl Default for DependencyConfig {
    fn default() -> Self {
        Self {
            window_days: default_window_days(),
        }
    }
}

impl DependencyConfig {
    pub fn window(&self) -> std::time::Duration {
        std::time::Duration::from_secs(u64::from(self.window_days) * 86_400)
    }
}

fn default_window_days() -> u32 {
    7
}
