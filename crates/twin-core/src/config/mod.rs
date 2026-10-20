mod dependencies;
mod retention;
mod template;

pub use dependencies::DependencyConfig;
pub use retention::{RetentionConfig, TwinConfig};
pub use template::DEFAULT_CONFIG_TOML;
