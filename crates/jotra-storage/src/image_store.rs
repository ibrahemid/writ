//! Saving an image pasted or dropped into a Markdown file (ADR-035).
//!
//! The name comes from `jotra_core::notes::image_import`; this module finds
//! the first form of it that is free, reuses a file already holding the same
//! bytes, and writes a new one without ever replacing what is there.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use jotra_core::notes::image_import::numbered_file_name;
pub use jotra_core::notes::image_import::MAX_NUMBERED_NAMES;
use thiserror::Error;

use crate::atomic::{write_new, AtomicWriteError};
use crate::paths::strip_verbatim_prefix;

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
    /// The folder, or a link on the way to it, resolves outside the folder it
    /// has to stay in.
    #[error("the image folder resolves outside the file's folder")]
    OutsideRoot,
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
/// file watcher, and `write_lost` runs with it after a write that did not
/// land, so the caller can take that mark back.
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
    mut write_lost: impl FnMut(&Path),
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
        let written = write_new(&path, bytes);
        if written.is_err() {
            write_lost(&path);
        }
        match written {
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

/// Makes `folder` one part at a time under `root`, a resolved folder, and
/// returns where it resolves, refusing it when it or a link on the way to it
/// resolves outside `root`.
///
/// `root` and `folder` may each carry the `\\?\` prefix Windows
/// canonicalisation adds or not; both are compared without it, and the folder
/// comes back without it ([`strip_verbatim_prefix`]).
///
/// # Errors
///
/// [`ImageSaveError::OutsideRoot`] when `folder` is not under `root` or a part
/// of it resolves outside, [`ImageSaveError::FolderNotWritable`] when a part
/// cannot be made or is not a folder, and [`ImageSaveError::Io`] for any
/// other failure.
pub fn make_folder_within(root: &Path, folder: &Path) -> Result<PathBuf, ImageSaveError> {
    let root = strip_verbatim_prefix(root.to_path_buf());
    let folder = strip_verbatim_prefix(folder.to_path_buf());
    let rest = folder
        .strip_prefix(&root)
        .map_err(|_| ImageSaveError::OutsideRoot)?;
    let mut current = root.clone();
    for part in rest.components() {
        let std::path::Component::Normal(part) = part else {
            return Err(ImageSaveError::OutsideRoot);
        };
        let next = current.join(part);
        match fs::create_dir(&next) {
            Ok(()) => {}
            Err(err) if err.kind() == io::ErrorKind::AlreadyExists => {}
            Err(err) if is_permission_error(&err) => return Err(ImageSaveError::FolderNotWritable),
            Err(err) => return Err(ImageSaveError::Io(err)),
        }
        let resolved = strip_verbatim_prefix(fs::canonicalize(&next)?);
        if !resolved.starts_with(&root) {
            return Err(ImageSaveError::OutsideRoot);
        }
        if !fs::metadata(&resolved)?.is_dir() {
            return Err(ImageSaveError::FolderNotWritable);
        }
        current = resolved;
    }
    Ok(current)
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
