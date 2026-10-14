//! Gathering a context pack: which files a source names, read through the
//! notes host with read access alone, and the text of open tabs in place of
//! what their files hold until autosave catches up.
//!
//! The pack itself (preparation, limits, the estimate) is
//! [`writ_core::prompt::pack`]; this module only finds and reads the files.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use writ_core::config::FileExtension;
use writ_core::notes::host::{Capability, HostError, NoteHost, MAX_NOTE_BYTES};
use writ_core::prompt::pack::{ContextPack, PackBuilder, SkipReason};

use crate::note_host::NoteHostImpl;
use crate::notes_index::{index_key, is_dataless, NotesIndexStore};

/// The files a pack is made of.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PackSource {
    /// One file and every file its links resolve to, in the order the links
    /// are written.
    Linked {
        /// The file, by its path on disk.
        path: String,
    },
    /// Every file carrying a tag, matched whole as the Tags list matches it.
    Tag {
        /// The tag, without its `#`.
        tag: String,
    },
    /// Every file in a folder and the folders under it, in path order.
    Folder {
        /// The folder, by its path on disk.
        path: String,
    },
}

/// The text an open tab holds that its file may not hold yet.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
pub struct LiveText {
    /// The file the tab shows, by its path on disk.
    pub path: String,
    /// What the tab holds.
    pub text: String,
}

/// Why no pack could be made.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ContextPackError {
    /// The source names a file or folder outside the folder Writ indexes.
    #[error("{path} is outside the folder")]
    OutsideFolder {
        /// The path the source named.
        path: String,
    },
    /// The folder itself could not be opened for reading.
    #[error("the folder could not be opened")]
    FolderUnavailable,
    /// The index the source is looked up in could not be read.
    #[error("the file index could not be read: {0}")]
    Index(String),
}

/// The index key of the folder with a trailing separator, so a folder never
/// claims a sibling whose name it starts with.
fn under(folder_key: &str) -> String {
    if folder_key.ends_with(std::path::MAIN_SEPARATOR) {
        folder_key.to_string()
    } else {
        format!("{folder_key}{}", std::path::MAIN_SEPARATOR)
    }
}

fn inside(root_key: &str, key: &str) -> bool {
    key.starts_with(&under(root_key))
}

/// The files `source` names, by index key, in pack order.
pub fn pack_paths(
    index: &NotesIndexStore,
    notes_root: &Path,
    source: &PackSource,
) -> Result<Vec<String>, ContextPackError> {
    let root_key = index_key(notes_root);
    let index_error =
        |error: crate::errors::StorageError| ContextPackError::Index(error.to_string());
    match source {
        PackSource::Linked { path } => {
            let key = index_key(Path::new(path));
            if !inside(&root_key, &key) {
                return Err(ContextPackError::OutsideFolder { path: path.clone() });
            }
            let mut seen: HashSet<String> = HashSet::from([key.clone()]);
            let mut paths = vec![key.clone()];
            for link in index.links_from(&key).map_err(index_error)? {
                if let Some(target) = link.to_path {
                    if seen.insert(target.clone()) {
                        paths.push(target);
                    }
                }
            }
            Ok(paths)
        }
        PackSource::Tag { tag } => {
            let tag = tag.trim().trim_start_matches('#');
            if tag.is_empty() {
                return Ok(Vec::new());
            }
            index.paths_for_tag(tag).map_err(index_error)
        }
        PackSource::Folder { path } => {
            let folder_key = index_key(Path::new(path));
            if folder_key != root_key && !inside(&root_key, &folder_key) {
                return Err(ContextPackError::OutsideFolder { path: path.clone() });
            }
            let prefix = under(&folder_key);
            let mut paths: Vec<String> = index
                .note_paths()
                .map_err(index_error)?
                .into_iter()
                .filter(|key| key.starts_with(&prefix))
                .collect();
            paths.sort();
            Ok(paths)
        }
    }
}

/// The path a pack names a file by: relative to the folder, `/`-separated.
fn display_path(root_key: &str, key: &str) -> String {
    key.strip_prefix(&under(root_key))
        .unwrap_or(key)
        .replace('\\', "/")
}

fn skip_reason(error: &HostError) -> SkipReason {
    match error {
        HostError::TooLarge { .. } => SkipReason::TooLarge,
        HostError::NotText { .. } => SkipReason::NotText,
        HostError::NotDownloaded { .. } => SkipReason::NotDownloaded,
        _ => SkipReason::Unreadable,
    }
}

/// Makes the pack `source` names.
///
/// Files are read only while the pack has room, so a folder of thousands
/// costs the reads of the files it carries. A file in the cloud and not on
/// this computer is listed, not downloaded.
pub fn build_context_pack(
    index: &NotesIndexStore,
    notes_root: &Path,
    source: &PackSource,
    live: &[LiveText],
) -> Result<ContextPack, ContextPackError> {
    let paths = pack_paths(index, notes_root, source)?;
    let root_key = index_key(notes_root);
    let host = NoteHostImpl::open(
        notes_root,
        None,
        [Capability::ReadNote].into_iter().collect(),
        FileExtension::Txt,
    )
    .map_err(|_| ContextPackError::FolderUnavailable)?;
    let live: HashMap<String, &str> = live
        .iter()
        .map(|tab| (index_key(Path::new(&tab.path)), tab.text.as_str()))
        .collect();

    let mut pack = PackBuilder::new();
    for key in &paths {
        if pack.is_full() {
            pack.omit();
            continue;
        }
        let shown = display_path(&root_key, key);
        if let Some(text) = live.get(key) {
            if text.len() as u64 > MAX_NOTE_BYTES {
                pack.skip(&shown, SkipReason::TooLarge);
            } else {
                pack.add(&shown, text);
            }
            continue;
        }
        if is_dataless(Path::new(key)) {
            pack.skip(&shown, SkipReason::NotDownloaded);
            continue;
        }
        match host.read_note(key) {
            Ok(content) => pack.add(&shown, &content.text),
            Err(error) => pack.skip(&shown, skip_reason(&error)),
        }
    }
    Ok(pack.finish())
}
