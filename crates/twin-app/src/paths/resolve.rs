use std::path::{Path, PathBuf};

use crate::error::PathError;
use crate::paths::TwinLayout;

#[derive(Debug, Clone)]
pub struct CommandPaths {
    pub layout: TwinLayout,
    pub config_path: PathBuf,
    pub db_path: PathBuf,
}

pub fn resolve_command_paths(config_override: Option<&Path>) -> Result<CommandPaths, PathError> {
    let layout = TwinLayout::from_xdg()?;
    let config_path = layout.config_file(config_override);
    let db_path = layout.db_file();
    Ok(CommandPaths {
        layout,
        config_path,
        db_path,
    })
}
