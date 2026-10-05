use std::path::{Path, PathBuf};

use writ_core::config::WritConfig;

use crate::atomic::write_atomic;
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
        let config = WritConfig::parse(&contents)?;
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

    /// Writes already-serialized config `contents` to disk, creating the
    /// folder that holds it if needed.
    ///
    /// The bytes go to a sibling temp file that is renamed over the config
    /// ([`write_atomic`]), so a crash or a reader in the middle of a write
    /// finds the previous settings or the new ones, never an empty or partial
    /// file. A `config.toml` that is a symlink, such as one into a dotfiles
    /// checkout, is resolved first and its target replaced, so the link
    /// survives. A hard-linked one is refused rather than split from its
    /// other name ([`AtomicWriteError::HardLinked`]).
    ///
    /// The rename gives the config a new inode, which is why the app's
    /// watcher follows the folder rather than the file. It fingerprints this
    /// exact `contents` before the write and reads the file back after the
    /// debounce window, so it recognises the write as Writ's own and never
    /// echoes it as an external change (`commands::config::persist_config`
    /// and the watcher handler in the app crate).
    ///
    /// # Errors
    ///
    /// [`StorageError::AtomicWrite`] when the config cannot be replaced in one
    /// step, and [`StorageError::Io`] when its folder cannot be created or its
    /// link cannot be read. The file on disk is unchanged in every case.
    ///
    /// [`AtomicWriteError::HardLinked`]: crate::atomic::AtomicWriteError::HardLinked
    /// [`StorageError::AtomicWrite`]: crate::errors::StorageError::AtomicWrite
    /// [`StorageError::Io`]: crate::errors::StorageError::Io
    pub fn write_serialized(&self, contents: &str) -> StorageResult<()> {
        let target = self.write_target()?;
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_atomic(&target, contents.as_bytes())?;
        Ok(())
    }

    /// The file a write replaces: the config path itself, or the file it
    /// links to.
    ///
    /// [`write_atomic`] replaces a symlink rather than writing through it, so
    /// the link is resolved here. A link whose target does not exist yet
    /// cannot be canonicalised; it is read one level and joined onto the
    /// folder holding the link, which is how the filesystem reads a relative
    /// link.
    fn write_target(&self) -> StorageResult<PathBuf> {
        let is_link = std::fs::symlink_metadata(&self.path)
            .map(|entry| entry.file_type().is_symlink())
            .unwrap_or(false);
        if !is_link {
            return Ok(self.path.clone());
        }
        match std::fs::canonicalize(&self.path) {
            Ok(target) => Ok(target),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let link = std::fs::read_link(&self.path)?;
                Ok(match self.path.parent() {
                    Some(folder) => folder.join(link),
                    None => link,
                })
            }
            Err(e) => Err(e.into()),
        }
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
