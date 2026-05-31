use std::fs;
use std::path::Path;

use twin_core::config::{TwinConfig, DEFAULT_CONFIG_TOML};
use twin_core::error::ConfigError;

pub fn read(path: &Path) -> Result<TwinConfig, ConfigError> {
    let contents = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&contents).map_err(|source| ConfigError::Parse { source })
}

pub fn sync_default_template(path: &Path, force: bool) -> Result<(bool, bool), ConfigError> {
    if !path.exists() {
        write_default(path)?;
        return Ok((true, false));
    }

    if force {
        write_default(path)?;
        return Ok((false, true));
    }

    let contents = fs::read_to_string(path).map_err(|source| ConfigError::Read {
        path: path.display().to_string(),
        source,
    })?;

    if contents == DEFAULT_CONFIG_TOML {
        return Ok((false, false));
    }

    if is_default_values(&contents) {
        write_default(path)?;
        return Ok((false, true));
    }

    Ok((false, false))
}

fn is_default_values(contents: &str) -> bool {
    match toml::from_str::<TwinConfig>(contents) {
        Ok(cfg) => cfg == TwinConfig::default(),
        Err(_) => false,
    }
}

fn write_default(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| ConfigError::Write {
            path: parent.display().to_string(),
            source,
        })?;
    }
    fs::write(path, DEFAULT_CONFIG_TOML).map_err(|source| ConfigError::Write {
        path: path.display().to_string(),
        source,
    })?;
    Ok(())
}
