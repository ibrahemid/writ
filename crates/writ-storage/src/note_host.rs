//! The one implementation of the note host surface (ADR-032).
//!
//! Everything a program is allowed to do to the notes folder happens here: the
//! folder walk, the index reads, and the three writes through the guarded
//! facade. Two consumers hold it, the tool surface a connected program calls
//! and the chat pane, and neither reaches past it.
//!
//! Every method opens with its capability check, so a call the consumer's
//! [`PermissionSet`] does not cover resolves no path, stats no file and asks
//! the index nothing. That ordering is the whole of the sandbox: a check that
//! ran after the resolution would already have told a caller whether a path
//! outside the folder exists.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use writ_core::hash::{digest_hex, Sha256Digest};
use writ_core::notes::containment::{resolve_for_containment, resolve_inside};
use writ_core::notes::guard::{is_not_downloaded, DiskState};
use writ_core::notes::host::{
    Capability, FolderTag, HostError, NoteBacklink, NoteContent, NoteFacts, NoteHit, NoteHost,
    NoteLink, NoteSummary, PermissionSet, RenameReceipt, WriteReceipt, MAX_NOTE_BYTES,
};
use writ_core::notes::WriteOrigin;

use crate::buffer_store::{dataless_flags, read_disk_state};
use crate::database::migrations::binary_schema_version;
use crate::errors::StorageError;
use crate::guarded::{
    create_note_guarded, keep_versions, write_note_guarded, ConflictPolicy, CreateNote, DiskRead,
    GuardedWrite, TakenName, WriteCapture,
};
use crate::note_history::NoteHistoryStore;
use crate::notes_index::{self, NotesIndexStore};
use crate::paths::relative_slug;

/// The extension a listing counts as a note.
const NOTE_EXTENSION: &str = "md";

/// The notes folder, the index over it, and what one consumer may ask of both.
///
/// The index sits behind an [`Arc`] so a process that serves several approvals
/// opens the database once: [`NoteHostImpl::with_permissions`] hands out a
/// second handle over the same folder and the same connection.
pub struct NoteHostImpl<'a> {
    notes_root: PathBuf,
    index: Option<Arc<NotesIndexStore>>,
    permissions: PermissionSet,
    history: Option<&'a NoteHistoryStore>,
}

impl std::fmt::Debug for NoteHostImpl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteHostImpl")
            .field("notes_root", &self.notes_root)
            .field("index", &self.index.is_some())
            .field("permissions", &self.permissions)
            .field("history", &self.history.is_some())
            .finish_non_exhaustive()
    }
}

impl<'a> NoteHostImpl<'a> {
    /// Opens the folder at `notes_root` and, if there is one to open, the index
    /// at `db_path`.
    ///
    /// An index that is absent, unreadable, or written to another schema
    /// version leaves the host without one rather than failing: the methods
    /// that read the folder still answer, and the rest say the index is not
    /// there ([`HostError::IndexUnavailable`]).
    ///
    /// The root is resolved once here so every containment check compares two
    /// paths the filesystem spells the same way.
    pub fn open(
        notes_root: &Path,
        db_path: Option<&Path>,
        permissions: PermissionSet,
    ) -> Result<Self, HostError> {
        let resolved = resolve_for_containment(notes_root)
            .filter(|root| root.is_dir())
            .ok_or_else(|| HostError::NotFound {
                path: notes_root.display().to_string(),
            })?;
        Ok(Self {
            notes_root: resolved,
            index: db_path.and_then(open_index).map(Arc::new),
            permissions,
            history: None,
        })
    }

    /// A second handle over the same folder and index, holding `permissions`.
    pub fn with_permissions(&self, permissions: PermissionSet) -> NoteHostImpl<'a> {
        NoteHostImpl {
            notes_root: self.notes_root.clone(),
            index: self.index.clone(),
            permissions,
            history: self.history,
        }
    }

    /// The same handle, capturing every write it makes into `history`.
    ///
    /// The app passes its store; a process that keeps no history passes `None`
    /// and the writes are made without one.
    pub fn with_history(mut self, history: Option<&'a NoteHistoryStore>) -> Self {
        self.history = history;
        self
    }

    /// The folder every path argument is checked against.
    pub fn notes_root(&self) -> &Path {
        &self.notes_root
    }

    /// Whether the index answered when the host was opened.
    pub fn has_index(&self) -> bool {
        self.index.is_some()
    }

    /// What this handle may ask for.
    pub fn permissions(&self) -> &PermissionSet {
        &self.permissions
    }

    /// The first line of every method.
    fn permit(&self, capability: Capability) -> Result<(), HostError> {
        if self.permissions.contains(capability) {
            return Ok(());
        }
        Err(HostError::NotPermitted { capability })
    }

    /// The index, or [`HostError::IndexUnavailable`] when there is none.
    fn index(&self) -> Result<&NotesIndexStore, HostError> {
        self.index.as_deref().ok_or(HostError::IndexUnavailable)
    }

    /// The file a path argument names, refusing anything the folder does not
    /// hold.
    ///
    /// A path that is not absolute is read from the notes folder. Joining
    /// happens before resolution, so `../` in a relative argument is walked and
    /// refused like any other way out. Resolution happens before the file is
    /// opened, so a symlink out of the folder is refused rather than followed
    /// (ADR-031 rule 3.7).
    fn note_file(&self, path: &str) -> Result<PathBuf, HostError> {
        let given = Path::new(path);
        let candidate = if given.is_absolute() {
            given.to_path_buf()
        } else {
            self.notes_root.join(given)
        };
        let file = resolve_inside(&self.notes_root, &candidate).ok_or_else(|| {
            HostError::OutsideNotesFolder {
                path: path.to_string(),
            }
        })?;
        if !file.is_file() {
            return Err(HostError::NotFound {
                path: path.to_string(),
            });
        }
        Ok(file)
    }

    /// The index key of the note a path argument names.
    fn note_key(&self, path: &str) -> Result<String, HostError> {
        Ok(notes_index::index_key(&self.note_file(path)?))
    }

    /// Everything the index holds about one note, read once for the two slices
    /// a consumer cuts out of it (ADR-036 section 2).
    fn facts(&self, path: &str) -> Result<crate::notes_index::NoteFactsRow, HostError> {
        let index = self.index()?;
        let key = self.note_key(path)?;
        index.facts(&key).map_err(|_| HostError::IndexUnavailable)
    }
}

impl NoteHost for NoteHostImpl<'_> {
    fn list_notes(
        &self,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NoteSummary>, HostError> {
        self.permit(Capability::ListNotes)?;

        let mut notes = Vec::new();
        for entry in crate::workspace_search::build_walk(&self.notes_root).build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            if writ_core::workspace::path_has_ignored_name(&self.notes_root, path) {
                continue;
            }
            if !path
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTE_EXTENSION))
            {
                continue;
            }
            let Some(relative) = relative_slug(&self.notes_root, path) else {
                continue;
            };
            let key = notes_index::index_key(path);
            if let Some(prefix) = prefix {
                if !relative.starts_with(prefix) && !key.starts_with(prefix) {
                    continue;
                }
            }
            notes.push(NoteSummary {
                name: writ_core::notes::note_display_name(&key),
                path: key,
                bytes: std::fs::metadata(path).map(|meta| meta.len()).unwrap_or(0),
            });
        }
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        notes.truncate(limit);
        Ok(notes)
    }

    fn read_note(&self, path: &str) -> Result<NoteContent, HostError> {
        self.permit(Capability::ReadNote)?;
        let file = self.note_file(path)?;

        let bytes = std::fs::metadata(&file)
            .map_err(|_| HostError::NotFound {
                path: path.to_string(),
            })?
            .len();
        if bytes > MAX_NOTE_BYTES {
            return Err(HostError::TooLarge {
                path: path.to_string(),
                bytes,
            });
        }
        let read = std::fs::read(&file).map_err(|_| HostError::Unreadable {
            path: path.to_string(),
        })?;
        let text = String::from_utf8(read).map_err(|_| HostError::NotText {
            path: path.to_string(),
        })?;
        Ok(NoteContent {
            path: notes_index::index_key(&file),
            bytes,
            hash: writ_core::hash::sha256_hex(text.as_bytes()),
            text,
        })
    }

    fn note_summary(&self, path: &str) -> Result<NoteSummary, HostError> {
        self.permit(Capability::ReadNote)?;
        let file = self.note_file(path)?;

        let bytes = std::fs::metadata(&file)
            .map_err(|_| HostError::Unreadable {
                path: path.to_string(),
            })?
            .len();
        let key = notes_index::index_key(&file);
        Ok(NoteSummary {
            name: writ_core::notes::note_display_name(&key),
            path: key,
            bytes,
        })
    }

    fn search_notes(&self, query: &str, limit: usize) -> Result<Vec<NoteHit>, HostError> {
        self.permit(Capability::SearchNotes)?;
        let index = self.index()?;

        let Some(expression) = writ_core::search::to_prefix_match(query) else {
            return Ok(Vec::new());
        };
        let terms = writ_core::search::search_terms(query);
        let hits = index
            .search_hits(&expression, &terms, limit)
            .map_err(|_| HostError::IndexUnavailable)?;

        Ok(hits
            .into_iter()
            .map(|hit| {
                let path = hit.path.unwrap_or_default();
                NoteHit {
                    name: writ_core::notes::note_display_name(&path),
                    path,
                    line: hit.line,
                    excerpt: hit
                        .snippet
                        .into_iter()
                        .map(|segment| segment.text)
                        .collect(),
                }
            })
            .collect())
    }

    fn note_links(&self, path: &str) -> Result<Vec<NoteLink>, HostError> {
        self.permit(Capability::ReadIndex)?;
        let index = self.index()?;
        let key = self.note_key(path)?;

        let rows = index
            .links_from(&key)
            .map_err(|_| HostError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|row| NoteLink {
                target: row.to_target,
                resolved_path: row.to_path,
                kind: row.kind,
                line: row.line,
                column: row.col,
            })
            .collect())
    }

    fn note_backlinks(&self, path: &str) -> Result<Vec<NoteBacklink>, HostError> {
        self.permit(Capability::ReadIndex)?;
        let index = self.index()?;
        let key = self.note_key(path)?;

        let rows = index
            .backlinks(&key)
            .map_err(|_| HostError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|row| NoteBacklink {
                from_path: row.from_path,
                from_name: row.from_name,
                target: row.to_target,
                alias: row.alias,
                kind: row.kind,
                line: row.line,
                column: row.col,
                context: row.context,
                certainty: row.certainty.as_str().to_string(),
                candidates: row.candidates,
            })
            .collect())
    }

    fn note_facts(&self, path: &str) -> Result<NoteFacts, HostError> {
        self.permit(Capability::ReadIndex)?;
        let facts = self.facts(path)?;
        Ok(NoteFacts {
            properties: facts.properties,
            tags: facts.tags,
        })
    }

    fn folder_tags(&self) -> Result<Vec<FolderTag>, HostError> {
        self.permit(Capability::ReadIndex)?;
        let index = self.index()?;
        let rows = index.all_tags().map_err(|_| HostError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|(tag, notes)| FolderTag { tag, notes })
            .collect())
    }

    fn write_note(
        &self,
        path: &str,
        content: &str,
        last_known: Option<Sha256Digest>,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::WriteNote)?;
        let file = self.note_file(path)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5). The guard asks the same
        // question, and asking it here is what lets the state below be read
        // once and handed over rather than read again inside it.
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
        let on_disk = read_disk_state(&file).map_err(|_| HostError::Unreadable {
            path: path.to_string(),
        })?;
        // With a digest from the caller, only that is compared: `decide_save`
        // reads neither the length nor the modification time, and a caller that
        // read the note over a wire knows neither about the file it read.
        // Without one, what the file holds now stands in, which is the same
        // thing as having no expectation and also lets a write of the text the
        // note already holds be recognised and skipped.
        let last_known = match last_known {
            Some(hash) => Some(DiskState {
                hash,
                size: 0,
                mtime: None,
            }),
            None => on_disk,
        };
        let keep = self.history.map(keep_versions);
        let outcome = write_note_guarded(
            GuardedWrite {
                target: &file,
                bytes: content.as_bytes(),
                last_known,
                // Read once, above, from the same bytes the digests here
                // describe.
                on_disk: DiskRead::Read(on_disk),
                dataless: None,
                origin,
                on_conflict: ConflictPolicy::RefuseWithCopy,
                history: keep.as_ref().map(|hook| hook as &dyn Fn(WriteCapture<'_>)),
            },
            // No ignore stamp: a write made through this surface is meant to
            // reach the running app the way any other program's write does,
            // through the folder watcher (ADR-033).
            None,
        )
        .map_err(|error| write_error(path, error))?;
        Ok(WriteReceipt {
            path: notes_index::index_key(&file),
            bytes: outcome.disk_state.size,
            hash: digest_hex(outcome.disk_state.hash),
        })
    }

    fn create_note(
        &self,
        name: &str,
        content: &str,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::CreateNote)?;
        let stem = writ_core::notes::sanitize_title(name).ok_or(HostError::NameEmpty)?;
        // The dedupe is what a person who asked for a new note wants and the
        // wrong answer for a program: a caller that asked for `Launch` and got
        // `Launch 2` has put its text in a note it did not name. The facade
        // holds the folding rule for what "taken" means, so it is asked rather
        // than second-guessed: a check here against the exact path would be the
        // filesystem's answer, and a case-sensitive volume folds nothing.
        let keep = self.history.map(keep_versions);
        let minted = create_note_guarded(
            CreateNote {
                notes_root: &self.notes_root,
                stem: &stem,
                content,
                origin,
                on_taken_name: TakenName::Refuse,
                history: keep.as_ref().map(|hook| hook as &dyn Fn(WriteCapture<'_>)),
            },
            None,
        )
        .map_err(|error| write_error(name, error))?;
        // Read back rather than hashed here: the facade lands a minted note as
        // LF, so what the caller handed in is not always what the file holds.
        let state =
            read_disk_state(&minted)
                .ok()
                .flatten()
                .ok_or_else(|| HostError::Unwritable {
                    path: name.to_string(),
                })?;
        Ok(WriteReceipt {
            path: notes_index::index_key(&minted),
            bytes: state.size,
            hash: digest_hex(state.hash),
        })
    }

    fn rename_note(
        &self,
        path: &str,
        new_name: &str,
        origin: WriteOrigin,
    ) -> Result<RenameReceipt, HostError> {
        self.permit(Capability::RenameNote)?;
        let file = self.note_file(path)?;
        // Maps a separator to a space, so a new name that spells a path names a
        // file in the folder the note is already in and cannot walk out of it.
        let stem = writ_core::notes::rename_stem(&file, new_name).ok_or(HostError::NameEmpty)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5).
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
        let last_known = read_disk_state(&file).map_err(|_| HostError::Unreadable {
            path: path.to_string(),
        })?;
        let bytes = last_known.map_or(0, |state| state.size);
        let moved = crate::note_ops::rename_note(
            &file, &stem, last_known, origin,
            // No ignore stamp, for the reason `write_note` gives.
            None,
        )
        .map_err(|error| write_error(path, error))?;
        Ok(RenameReceipt {
            path: notes_index::index_key(&moved),
            previous_path: notes_index::index_key(&file),
            bytes,
        })
    }
}

/// A storage refusal as the surface's own, naming the path the caller wrote.
///
/// The digest the guard carries and the folder it names are the app's own
/// spellings of this machine, and neither is in the answer.
fn write_error(path: &str, error: StorageError) -> HostError {
    match error {
        StorageError::SourceChangedOnDisk { conflict_copy, .. } => HostError::Conflict {
            path: path.to_string(),
            conflict_copy,
        },
        StorageError::SourceNotDownloaded { .. } => HostError::NotDownloaded {
            path: path.to_string(),
        },
        StorageError::NoteNameEmpty => HostError::NameEmpty,
        StorageError::NoteNameTaken { name, .. } => HostError::NameTaken { name },
        _ => HostError::Unwritable {
            path: path.to_string(),
        },
    }
}

/// Opens the index read-only, or `None` when there is nothing to open.
///
/// The schema check is `writ`'s: a database older than this build has columns a
/// read may not find, and a newer one was written by a build that knows more.
/// Both are the same situation to a caller as an absent one, and none of the
/// three is repaired here.
fn open_index(db_path: &Path) -> Option<NotesIndexStore> {
    if !db_path.is_file() {
        return None;
    }
    let store = NotesIndexStore::open_read_only(db_path).ok()?;
    (store.schema_version().ok()? == binary_schema_version()).then_some(store)
}
