use std::path::PathBuf;

use crate::error::PathError;
use crate::paths::TwinLayout;

impl TwinLayout {
    pub fn from_xdg() -> Result<Self, PathError> {
        Ok(Self {
            data_dir: xdg_subdir(dirs::data_dir)?,
            config_dir: xdg_subdir(dirs::config_dir)?,
            state_dir: xdg_subdir(dirs::state_dir)?,
        })
    }
}

fn xdg_subdir(base: impl FnOnce() -> Option<PathBuf>) -> Result<PathBuf, PathError> {
    base().map(|dir| dir.join("twin")).ok_or(PathError::NoHome)
}
