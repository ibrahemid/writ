use std::path::{Path, PathBuf};

use writ_core::config::WritConfig;

use crate::atomic::write_atomic;
use crate::errors::StorageResult;

/// Most links a config write follows before it calls the chain a loop: the
/// limit Linux puts on resolving a single path.
const MAX_LINK_HOPS: usize = 40;

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
    /// checkout, is followed to the file at the end of its links and that
    /// file replaced, so every link survives. A hard-linked one is refused
    /// rather than split from its other name ([`AtomicWriteError::HardLinked`]).
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
    /// step, and [`StorageError::Io`] when its folder cannot be created, a link
    /// on the way cannot be read, or the links loop. The file on disk is
    /// unchanged in every case.
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

    /// The file a write replaces: the config path itself, or the file at the
    /// end of the links it leads through.
    ///
    /// [`write_atomic`] replaces a symlink rather than writing through it, so
    /// every link on the way is followed here and the first path that is not a
    /// link is the one replaced. Each hop is joined onto the folder holding the
    /// link it came from, which is how the filesystem reads a relative link.
    /// Following the chain hop by hop rather than canonicalising it treats a
    /// target that exists and one not written yet alike: a chain that ends in
    /// nothing has the file created at its end, and no link in it is replaced.
    /// A chain longer than [`MAX_LINK_HOPS`] is a loop and is refused.
    fn write_target(&self) -> StorageResult<PathBuf> {
        let mut current = self.path.clone();
        for _ in 0..MAX_LINK_HOPS {
            if !current.is_symlink() {
                return Ok(current);
            }
            let link = std::fs::read_link(&current)?;
            current = match current.parent() {
                Some(folder) => folder.join(link),
                None => link,
            };
        }
        // `ErrorKind::FilesystemLoop` is not stable at this crate's
        // rust-version, so a loop reads as a config path that leads to no file.
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "{} leads through more than {MAX_LINK_HOPS} links without reaching a file",
                self.path.display()
            ),
        )
        .into())
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
