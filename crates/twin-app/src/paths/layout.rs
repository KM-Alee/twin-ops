use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct TwinLayout {
    pub data_dir: PathBuf,
    pub config_dir: PathBuf,
    pub state_dir: PathBuf,
}

impl TwinLayout {
    pub fn isolated(base: impl AsRef<Path>) -> Self {
        let base = base.as_ref();
        Self {
            data_dir: base.join("data"),
            config_dir: base.join("config"),
            state_dir: base.join("state"),
        }
    }

    pub fn ensure_dirs(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.data_dir)?;
        std::fs::create_dir_all(&self.config_dir)?;
        std::fs::create_dir_all(&self.state_dir)?;
        Ok(())
    }

    pub fn config_file(&self, override_path: Option<&Path>) -> PathBuf {
        match override_path {
            Some(path) => path.to_path_buf(),
            None => self.config_dir.join("config.toml"),
        }
    }

    pub fn db_file(&self) -> PathBuf {
        self.data_dir.join("twin.db")
    }

    pub fn log_file(&self) -> PathBuf {
        self.state_dir.join("twin.log")
    }
}
