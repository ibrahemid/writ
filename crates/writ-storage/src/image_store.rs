//! Saving an image pasted or dropped into a Markdown file (ADR-035).
//!
//! The name comes from `writ_core::notes::image_import`; this module finds
//! the first form of it that is free, reuses a file already holding the same
//! bytes, and writes a new one without ever replacing what is there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use writ_core::notes::image_import::numbered_file_name;

use crate::atomic::{write_new, AtomicWriteError};

/// The most numbered names tried before a save gives up.
pub const MAX_NUMBERED_NAMES: u32 = 999;

/// An image on disk after a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SavedImage {
    /// A new file was written.
    Written {
        /// The file's name in the folder.
        name: String,
        /// The file's path.
        path: PathBuf,
    },
    /// A file in the folder already held these bytes and is used as it is.
    Reused {
        /// The file's name in the folder.
        name: String,
        /// The file's path.
        path: PathBuf,
    },
}

impl SavedImage {
    /// The file's name in the folder.
    pub fn name(&self) -> &str {
        match self {
            Self::Written { name, .. } | Self::Reused { name, .. } => name,
        }
    }

    /// The file's path.
    pub fn path(&self) -> &Path {
        match self {
            Self::Written { path, .. } | Self::Reused { path, .. } => path,
        }
    }
}

/// Why an image was not saved.
#[derive(Debug, Error)]
pub enum ImageSaveError {
    /// The folder could not be made or would not take a new file.
    #[error("the image folder cannot be written")]
    FolderNotWritable,
    /// Every name up to [`MAX_NUMBERED_NAMES`] is taken by other bytes.
    #[error("no free name for the image after {0} tries")]
    NoFreeName(u32),
    /// Anything else the filesystem reported.
    #[error("io error: {0}")]
    Io(#[from] io::Error),
}

/// Saves `bytes` in `folder` as `name`, or as the first `name-2`, `name-3`
/// that is free, making the folder when it is missing.
///
/// A regular file under one of those names that already holds `bytes` is
/// reused instead of written again. `before_write` runs with the path just
/// before each write, so the caller can mark the write as its own for the
/// file watcher.
///
/// # Errors
///
/// [`ImageSaveError::FolderNotWritable`] when the folder cannot be made or
/// written, [`ImageSaveError::NoFreeName`] when every numbered name is
/// taken, and [`ImageSaveError::Io`] for any other failure.
pub fn save_image(
    folder: &Path,
    name: &str,
    bytes: &[u8],
    mut before_write: impl FnMut(&Path),
) -> Result<SavedImage, ImageSaveError> {
    make_folder(folder)?;
    for number in 1..=MAX_NUMBERED_NAMES {
        let candidate = numbered_file_name(name, number);
        let path = folder.join(&candidate);
        if let Some(found) = existing_entry(&path)? {
            if found == Existing::File && holds_bytes(&path, bytes)? {
                return Ok(SavedImage::Reused {
                    name: candidate,
                    path,
                });
            }
            continue;
        }
        before_write(&path);
        match write_new(&path, bytes) {
            Ok(()) => {
                return Ok(SavedImage::Written {
                    name: candidate,
                    path,
                })
            }
            Err(AtomicWriteError::Io(err)) if err.kind() == io::ErrorKind::AlreadyExists => {
                continue
            }
            Err(AtomicWriteError::FolderNotWritable | AtomicWriteError::ReadOnly) => {
                return Err(ImageSaveError::FolderNotWritable)
            }
            Err(AtomicWriteError::Io(err)) if is_permission_error(&err) => {
                return Err(ImageSaveError::FolderNotWritable)
            }
            Err(AtomicWriteError::Io(err)) => return Err(ImageSaveError::Io(err)),
            Err(other @ AtomicWriteError::HardLinked { .. }) => {
                return Err(ImageSaveError::Io(io::Error::other(other.to_string())))
            }
        }
    }
    Err(ImageSaveError::NoFreeName(MAX_NUMBERED_NAMES))
}

#[derive(Debug, PartialEq, Eq)]
enum Existing {
    /// A regular file.
    File,
    /// A folder, a link, or anything else a name can hold.
    Other,
}

fn existing_entry(path: &Path) -> Result<Option<Existing>, ImageSaveError> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Ok(Some(Existing::File)),
        Ok(_) => Ok(Some(Existing::Other)),
        Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(err) => Err(ImageSaveError::Io(err)),
    }
}

fn holds_bytes(path: &Path, bytes: &[u8]) -> Result<bool, ImageSaveError> {
    let len = fs::metadata(path)?.len();
    if len != bytes.len() as u64 {
        return Ok(false);
    }
    Ok(fs::read(path)? == bytes)
}

fn make_folder(folder: &Path) -> Result<(), ImageSaveError> {
    match fs::create_dir_all(folder) {
        Ok(()) => {}
        Err(err) if is_permission_error(&err) => return Err(ImageSaveError::FolderNotWritable),
        Err(err) => return Err(ImageSaveError::Io(err)),
    }
    if !fs::metadata(folder)?.is_dir() {
        return Err(ImageSaveError::FolderNotWritable);
    }
    Ok(())
}

fn is_permission_error(err: &io::Error) -> bool {
    matches!(
        err.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}
