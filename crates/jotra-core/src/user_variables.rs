//! The environment variables a person sets for Jotra, read under the current
//! name first and the name from before the rename second (ADR-059 decision 5).

use std::ffi::OsString;

use crate::legacy_names;

/// The start of every environment variable name the app reads under the
/// current name.
pub const VARIABLE_PREFIX: &str = "JOTRA_";

/// A variable a person may export in a shell profile, a launch agent or a
/// wrapper script.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserVariable {
    /// The data folder (`JOTRA_DATA_DIR`).
    DataDir,
    /// The notes folder (`JOTRA_NOTES_DIR`).
    NotesDir,
    /// The app binary the command launches (`JOTRA_GUI_BIN`).
    GuiBin,
}

impl UserVariable {
    /// The name docs and generated wrappers write.
    pub fn current_name(self) -> &'static str {
        match self {
            Self::DataDir => "JOTRA_DATA_DIR",
            Self::NotesDir => "JOTRA_NOTES_DIR",
            Self::GuiBin => "JOTRA_GUI_BIN",
        }
    }

    /// The name from before the rename, still read.
    pub fn legacy_name(self) -> &'static str {
        match self {
            Self::DataDir => legacy_names::DATA_DIR_VAR,
            Self::NotesDir => legacy_names::NOTES_DIR_VAR,
            Self::GuiBin => legacy_names::GUI_BIN_VAR,
        }
    }

    /// Both names, in the order they are read.
    pub fn names(self) -> [&'static str; 2] {
        [self.current_name(), self.legacy_name()]
    }
}

/// Reads `variable` through `lookup`: the current name, then the old one.
///
/// A blank value counts as unset, so an exported-but-empty variable falls
/// through to the other name and then to whatever comes after both.
pub fn read_user_variable(
    variable: UserVariable,
    lookup: impl Fn(&str) -> Option<OsString>,
) -> Option<OsString> {
    variable
        .names()
        .into_iter()
        .filter_map(lookup)
        .find(|value| !value.to_string_lossy().trim().is_empty())
}
