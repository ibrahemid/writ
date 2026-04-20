use std::path::{Path, PathBuf};

use writ_core::config::WritConfig;

use crate::errors::StorageResult;

/// TOML-backed configuration store.
///
/// The store reads and writes [`WritConfig`] at a single path. A missing
/// file is treated as "use defaults" to keep fresh installs friction-free.
pub struct ConfigStore {
    path: PathBuf,
}

impl ConfigStore {
    /// Constructs a store rooted at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Reads the configuration file, returning defaults if it does not
    /// exist.
    pub fn read(&self) -> StorageResult<WritConfig> {
        if !self.path.exists() {
            return Ok(WritConfig::default());
        }
        let contents = std::fs::read_to_string(&self.path)?;
        let config: WritConfig = toml::from_str(&contents)?;
        Ok(config)
    }

    /// Writes the configuration to disk, creating the parent directory
    /// if needed.
    pub fn write(&self, config: &WritConfig) -> StorageResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let contents = toml::to_string(config)?;
        std::fs::write(&self.path, contents)?;
        Ok(())
    }

    /// Returns the filesystem path this store reads from and writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }
}
