//! Which data folder a launch uses, the index inside it, the name it was made
//! under, and the notes folder that follows from it (ADR-059 decisions 3 and 4).
//!
//! An install made before the rename keeps its data where it is: nothing is
//! moved, copied or renamed. The app, the command and the MCP server all ask
//! [`resolve_data_folder`], so the three always open the same index.

use std::path::{Path, PathBuf};

use crate::legacy_names;
use crate::notes::DEFAULT_NOTES_FOLDER;

/// The data folder's name under the home folder.
pub const DATA_FOLDER: &str = ".jotra";

/// The index file inside [`DATA_FOLDER`], and the name a new index takes.
pub const INDEX_FILE: &str = "jotra.db";

/// The name a data folder was made under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lineage {
    /// The home data folder made before the rename. Its notes default keeps
    /// the folder name it has always had.
    Legacy,
    /// The home data folder made under the current name, or a folder named by
    /// a data-folder variable.
    Current,
}

/// Where the data folder came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataFolderOrigin {
    /// A data-folder variable named it, and it is used as given.
    Variable,
    /// It sits under the home folder.
    Home,
}

/// The data folder a launch uses.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataFolder {
    /// The folder holding the index, the config and everything else Jotra keeps.
    pub folder: PathBuf,
    /// The index file inside [`DataFolder::folder`].
    pub index: PathBuf,
    /// The name the folder was made under.
    pub lineage: Lineage,
    /// Where the folder came from.
    pub origin: DataFolderOrigin,
    /// The notes folder when neither a variable nor the config names one
    /// (decision 4). Derived on every launch and never written to the config.
    pub notes_default: PathBuf,
}

impl DataFolder {
    /// Whether a data-folder variable named this folder.
    pub fn is_override(&self) -> bool {
        self.origin == DataFolderOrigin::Variable
    }
}

/// Why no data folder could be chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DataFolderError {
    /// No data-folder variable is set and no home folder could be resolved.
    #[error("no home folder is available and no data folder variable is set")]
    NoHome,
}

/// What [`resolve_data_folder`] decides from.
#[derive(Debug, Default, Clone, Copy)]
pub struct DataFolderSources<'a> {
    /// The value of the data-folder variable, already read under both names
    /// ([`crate::user_variables::read_user_variable`]).
    pub variable: Option<&'a Path>,
    /// The user's home folder.
    pub home: Option<&'a Path>,
}

/// Chooses the data folder by the five rules of ADR-059 decision 3, in order.
///
/// 1. A data-folder variable: the folder as given, [`Lineage::Current`]. Its
///    index is the current name if that file exists, else the old name if that
///    one does, else the current name.
/// 2. The current home folder when its index exists.
/// 3. The old home folder when its index exists.
/// 4. The current home folder when it exists, else the old one when it exists.
///    This covers an index deleted for a rebuild.
/// 5. The current home folder, which the caller creates.
///
/// `exists` answers whether a path is on disk, so the rules stay free of I/O
/// and a test can describe a home folder without making one.
pub fn resolve_data_folder(
    sources: DataFolderSources<'_>,
    exists: impl Fn(&Path) -> bool,
) -> Result<DataFolder, DataFolderError> {
    if let Some(folder) = sources.variable {
        return Ok(variable_data_folder(folder, &exists));
    }

    let home = sources.home.ok_or(DataFolderError::NoHome)?;
    let current = home.join(DATA_FOLDER);
    let legacy = home.join(legacy_names::DATA_FOLDER);
    let lineage = if exists(&current.join(INDEX_FILE)) {
        Lineage::Current
    } else if exists(&legacy.join(legacy_names::INDEX_FILE)) {
        Lineage::Legacy
    } else if exists(&current) {
        Lineage::Current
    } else if exists(&legacy) {
        Lineage::Legacy
    } else {
        Lineage::Current
    };
    Ok(home_data_folder(home, lineage))
}

/// The index a data folder holds: [`INDEX_FILE`] when it exists, else the
/// name from before the rename when that one does, else [`INDEX_FILE`].
///
/// This is rule 1's choice for a folder named by a variable. Once a launch has
/// opened its index, it also names the index of any folder the resolver chose.
pub fn index_in_folder(folder: &Path, exists: impl Fn(&Path) -> bool) -> PathBuf {
    [INDEX_FILE, legacy_names::INDEX_FILE]
        .into_iter()
        .map(|name| folder.join(name))
        .find(|path| exists(path))
        .unwrap_or_else(|| folder.join(INDEX_FILE))
}

fn variable_data_folder(folder: &Path, exists: &impl Fn(&Path) -> bool) -> DataFolder {
    let index = index_in_folder(folder, exists);
    // A folder named by a variable carries no name of its own, so an existing
    // subfolder under the old notes name is what marks one made before the
    // rename.
    let legacy_notes = folder.join(legacy_names::NOTES_FOLDER);
    let notes_default = if exists(&legacy_notes) {
        legacy_notes
    } else {
        folder.join(DEFAULT_NOTES_FOLDER)
    };
    DataFolder {
        folder: folder.to_path_buf(),
        index,
        lineage: Lineage::Current,
        origin: DataFolderOrigin::Variable,
        notes_default,
    }
}

fn home_data_folder(home: &Path, lineage: Lineage) -> DataFolder {
    let (folder, index, notes) = match lineage {
        Lineage::Current => (DATA_FOLDER, INDEX_FILE, DEFAULT_NOTES_FOLDER),
        Lineage::Legacy => (
            legacy_names::DATA_FOLDER,
            legacy_names::INDEX_FILE,
            legacy_names::NOTES_FOLDER,
        ),
    };
    let folder = home.join(folder);
    DataFolder {
        index: folder.join(index),
        folder,
        lineage,
        origin: DataFolderOrigin::Home,
        notes_default: home.join(notes),
    }
}

/// Every folder that holds, or may hold, Jotra's data on this machine: the
/// one in force and both home data folders.
///
/// A notes folder may hold none of them (decision 3). The one in force is
/// listed first; a home folder equal to it is not listed twice.
pub fn data_folders_on_machine(in_force: &Path, home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![in_force.to_path_buf()];
    if let Some(home) = home {
        for name in [DATA_FOLDER, legacy_names::DATA_FOLDER] {
            let folder = home.join(name);
            if !folders.contains(&folder) {
                folders.push(folder);
            }
        }
    }
    folders
}
