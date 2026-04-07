use std::path::{Path, PathBuf};

use writ_core::config::WritConfig;

use crate::errors::StorageResult;

pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn read(&self) -> StorageResult<WritConfig> {
        if !self.path.exists() {
            return Ok(WritConfig::default());
        }
        let contents = std::fs::read_to_string(&self.path)?;
        let config: WritConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn write(&self, config: &WritConfig) -> StorageResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string(config)?;
        std::fs::write(&self.path, contents)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
