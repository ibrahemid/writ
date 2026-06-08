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

    /// Serializes `config` to the exact TOML bytes that [`write`] will
    /// persist to disk.
    ///
    /// Exposed so callers can fingerprint the bytes for the file
    /// watcher's ignore set before the write hits disk, guaranteeing the
    /// fingerprint matches what the watcher later reads back.
    ///
    /// [`write`]: Self::write
    pub fn serialize(&self, config: &WritConfig) -> StorageResult<String> {
        Ok(toml::to_string(config)?)
    }

    /// Writes already-serialized config `contents` to disk in place,
    /// creating the parent directory if needed.
    ///
    /// The write is in-place (not via temp+rename) so the config file keeps
    /// its inode and the watcher's file-level watch survives the write on
    /// every notify backend. The watcher fingerprints this exact `contents`
    /// before the write and reads it back after the debounce window, so it
    /// recognizes the write as internal and never echoes it as an external
    /// change (see commands::config::update_config and the watcher handler).
    pub fn write_serialized(&self, contents: &str) -> StorageResult<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&self.path, contents)?;
        Ok(())
    }

    /// Serializes and atomically writes `config` to disk.
    pub fn write(&self, config: &WritConfig) -> StorageResult<()> {
        let contents = self.serialize(config)?;
        self.write_serialized(&contents)
    }

    /// Returns the filesystem path this store reads from and writes to.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("nested").join("config.toml"));
        let config = WritConfig::default();

        store.write(&config).unwrap();
        let read_back = store.read().unwrap();

        assert_eq!(read_back, config);
    }

    #[test]
    fn serialize_matches_bytes_written_to_disk() {
        let dir = tempdir().unwrap();
        let store = ConfigStore::new(dir.path().join("config.toml"));
        let config = WritConfig::default();

        let serialized = store.serialize(&config).unwrap();
        store.write(&config).unwrap();
        let on_disk = std::fs::read_to_string(store.path()).unwrap();

        assert_eq!(serialized, on_disk);
    }
}
