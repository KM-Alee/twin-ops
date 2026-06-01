use std::path::{Path, PathBuf};

use crate::error::PathError;
use crate::paths::TwinLayout;

impl TwinLayout {
    pub fn from_xdg() -> Result<Self, PathError> {
        if let Some(home) = sudo_invoker_home_dir() {
            return Ok(Self::from_home(&home));
        }
        Ok(Self {
            data_dir: xdg_subdir(dirs::data_dir)?,
            config_dir: xdg_subdir(dirs::config_dir)?,
            state_dir: xdg_subdir(dirs::state_dir)?,
        })
    }

    fn from_home(home: &Path) -> Self {
        Self {
            data_dir: home.join(".local/share/twin"),
            config_dir: home.join(".config/twin"),
            state_dir: home.join(".local/state/twin"),
        }
    }
}

fn xdg_subdir(base: impl FnOnce() -> Option<PathBuf>) -> Result<PathBuf, PathError> {
    base().map(|dir| dir.join("twin")).ok_or(PathError::NoHome)
}

/// When `twin` runs under `sudo`, `HOME` points at root but the database usually
/// lives in the invoking user's XDG dirs. Use `SUDO_USER` / `SUDO_UID` to find it.
pub(crate) fn sudo_invoker_home_dir() -> Option<PathBuf> {
    std::env::var_os("SUDO_UID")?;
    let user = std::env::var_os("SUDO_USER")?;
    if user.is_empty() || user == "root" {
        return None;
    }
    let user = user.to_str()?;
    home_dir_for_username(user)
}

fn home_dir_for_username(user: &str) -> Option<PathBuf> {
    for base in ["/home", "/var/home"] {
        let candidate = PathBuf::from(base).join(user);
        if candidate.is_dir() {
            return Some(candidate);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn home_dir_for_username_finds_existing_home() {
        let Some(home) = dirs::home_dir() else {
            return;
        };
        let Some(name) = home.file_name().and_then(|n| n.to_str()) else {
            return;
        };
        let found = home_dir_for_username(name).expect("home");
        assert_eq!(found, home);
    }
}
