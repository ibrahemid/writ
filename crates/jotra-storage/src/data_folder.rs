//! The data folder and its index, found on disk (ADR-059 decision 3).
//!
//! The rules live in [`jotra_core::data_folder`]; this module answers them
//! against the filesystem. Nothing here creates, moves or renames anything.

use std::path::{Path, PathBuf};

use jotra_core::data_folder::{
    index_in_folder, resolve_data_folder, DataFolder, DataFolderError, DataFolderSources,
};

/// The data folder this launch uses, chosen by
/// [`jotra_core::data_folder::resolve_data_folder`] against what is on disk.
///
/// The caller creates the folder when it is absent.
pub fn locate_data_folder(sources: DataFolderSources<'_>) -> Result<DataFolder, DataFolderError> {
    resolve_data_folder(sources, Path::exists)
}

/// The index file inside `folder`, by
/// [`jotra_core::data_folder::index_in_folder`] against what is on disk.
pub fn locate_index(folder: &Path) -> PathBuf {
    index_in_folder(folder, Path::exists)
}
