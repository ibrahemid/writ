//! The one implementation of the note host surface (ADR-032).
//!
//! Everything a program is allowed to do to the notes folder happens here: the
//! folder walk, the index reads, and the three writes through the guarded
//! facade. Two consumers hold it, the tool surface a connected program calls
//! and the chat pane, and neither reaches past it.
//!
//! Every method opens with its capability check, so a call the consumer's
//! [`writ_core::notes::host::PermissionSet`] does not cover resolves no path, stats no file and asks
//! the index nothing. That ordering is the whole of the sandbox: a check that
//! ran after the resolution would already have told a caller whether a path
//! outside the folder exists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use tracing::warn;
use writ_core::config::FileExtension;
use writ_core::hash::{digest_hex, Sha256Digest};
use writ_core::notes::containment::{resolve_for_containment, resolve_inside};
use writ_core::notes::guard::{is_not_downloaded, DiskState};
use writ_core::notes::host::{
    Capability, FolderTag, HostError, IndexEntry, LastKnown, NoteBacklink, NoteContent, NoteFacts,
    NoteHit, NoteHost, NoteLink, NoteSummary, PassageReceipt, PermissionSet, RenameReceipt,
    WriteReceipt, MAX_NOTE_BYTES,
};
use writ_core::notes::passage::{replace_passage, PassageError};
use writ_core::notes::WriteOrigin;
use writ_core::write_review::{verdict, ProposedChange, Refused, WriteProposal, WriteReview};

use crate::buffer_store::{dataless_flags, read_disk_state};
use crate::database::migrations::binary_schema_version;
use crate::errors::StorageError;
use crate::guarded::{
    create_note_guarded, keep_versions, write_note_guarded, ConflictPolicy, CreateNote, DiskRead,
    GuardedWrite, TakenName, WriteCapture,
};
use crate::note_history::NoteHistoryStore;
use crate::notes_index::{self, NotesIndexStore};
use crate::paths::{file_name_only, relative_slug};
use crate::write_lock::WriteLock;

/// How many times an append or a passage replace reads the note again when it
/// changed between the read and the rename.
pub const APPEND_ATTEMPTS: usize = 3;

/// The notes folder, the index over it, and what one consumer may ask of both.
///
/// The index sits behind an [`Arc`] so a process that serves several approvals
/// opens the database once: [`NoteHostImpl::with_permissions`] hands out a
/// second handle over the same folder and the same connection. What the host
/// has seen each note hold is shared the same way, so a read made through one
/// handle is what a write through the next one compares against
/// ([`LastKnown::LastSeen`]).
pub struct NoteHostImpl<'a> {
    notes_root: PathBuf,
    index: Option<Arc<NotesIndexStore>>,
    permissions: PermissionSet,
    history: Option<&'a NoteHistoryStore>,
    write_lock: Option<WriteLock>,
    default_extension: FileExtension,
    seen: Arc<Mutex<HashMap<String, Sha256Digest>>>,
    review: Option<Reviewer<'a>>,
}

/// Who a write made through a handle is held for, and whose write it is.
#[derive(Clone)]
struct Reviewer<'a> {
    review: &'a dyn WriteReview,
    program: String,
}

impl std::fmt::Debug for NoteHostImpl<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("NoteHostImpl")
            .field("notes_root", &self.notes_root)
            .field("index", &self.index.is_some())
            .field("permissions", &self.permissions)
            .field("history", &self.history.is_some())
            .field("write_lock", &self.write_lock)
            .field("default_extension", &self.default_extension)
            .field("seen", &self.seen().len())
            .field("review", &self.review.is_some())
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
    ///
    /// `default_extension` is the format [`NoteHost::create_note`] mints in
    /// when the name it is handed spells none.
    /// A consumer reads it from its own config ([`writ_core::config::FilesConfig`]):
    /// a note a connected program makes is a file Writ mints and follows the
    /// same rule as the rest (ADR-041 §2).
    pub fn open(
        notes_root: &Path,
        db_path: Option<&Path>,
        permissions: PermissionSet,
        default_extension: FileExtension,
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
            write_lock: None,
            default_extension,
            seen: Arc::default(),
            review: None,
        })
    }

    /// A handle over `notes_root`, minting in `default_extension`, with the
    /// index at `db_path` opened again, holding what this one holds.
    ///
    /// What this handle has seen each note hold carries over, so a long-lived
    /// consumer that follows a moved folder or an index that appeared after it
    /// started does not forget what it read: a write compares against the same
    /// text either way ([`LastKnown::LastSeen`]). The folder is resolved and
    /// the index opened as [`NoteHostImpl::open`] does, with the same answers.
    pub fn reopen(
        &self,
        notes_root: &Path,
        db_path: Option<&Path>,
        default_extension: FileExtension,
    ) -> Result<NoteHostImpl<'a>, HostError> {
        let opened = NoteHostImpl::open(
            notes_root,
            db_path,
            self.permissions.clone(),
            default_extension,
        )?;
        Ok(NoteHostImpl {
            history: self.history,
            write_lock: self.write_lock.clone(),
            seen: Arc::clone(&self.seen),
            ..opened
        })
    }

    /// The format a note this handle mints carries when its name spells none.
    pub fn default_extension(&self) -> FileExtension {
        self.default_extension
    }

    /// A second handle over the same folder and index, holding `permissions`.
    ///
    /// It sees what this handle has seen and the other way round: a note read
    /// through either is a note both have read.
    pub fn with_permissions(&self, permissions: PermissionSet) -> NoteHostImpl<'a> {
        NoteHostImpl {
            notes_root: self.notes_root.clone(),
            index: self.index.clone(),
            permissions,
            history: self.history,
            write_lock: self.write_lock.clone(),
            default_extension: self.default_extension,
            seen: Arc::clone(&self.seen),
            review: self.review.clone(),
        }
    }

    /// The same handle, capturing what it replaces into `history`.
    ///
    /// The app passes its store and so does `writ mcp`, which opens the same
    /// `history.db` (ADR-031 rule 1.3); a caller that keeps no history passes
    /// `None` and the writes are made without one. A minted note has no text to
    /// keep a version of, so [`NoteHost::create_note`] captures nothing either
    /// way.
    pub fn with_history(mut self, history: Option<&'a NoteHistoryStore>) -> Self {
        self.history = history;
        self
    }

    /// The same handle, holding the data folder's write lock across the last
    /// read and the rename of each write it makes ([`crate::write_lock`]).
    ///
    /// The app holds the same lock around its saves, so a write made through
    /// this handle and a save in the app never land between each other's
    /// compare and rename (AM-40). Every handle made from this one holds it.
    pub fn with_write_lock(mut self, lock: Option<WriteLock>) -> Self {
        self.write_lock = lock;
        self
    }

    /// The same handle, holding each write it would make for `review` to
    /// answer, as `program`'s write (ADR-053).
    ///
    /// The bytes are built first and shown with what the file holds; only
    /// [`writ_core::write_review::ReviewAnswer::Apply`] lands them, still
    /// under the guard and against the bytes the change was built from, so a
    /// file that changed while the person looked is refused rather than
    /// written over, and nothing is set beside it. Nothing is held while the
    /// lock is: it is taken after the answer. `None` writes without asking.
    pub fn with_review(mut self, review: Option<&'a dyn WriteReview>, program: &str) -> Self {
        self.review = review.map(|review| Reviewer {
            review,
            program: program.to_string(),
        });
        self
    }

    /// The folder every path argument is checked against.
    pub fn notes_root(&self) -> &Path {
        &self.notes_root
    }

    /// Whether the index answered when this handle was opened.
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

    /// What this host has seen each note hold, by the note's index key.
    fn seen(&self) -> std::sync::MutexGuard<'_, HashMap<String, Sha256Digest>> {
        self.seen
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Records that the note at `key` holds the text `digest` names, as this
    /// host last saw it.
    fn saw(&self, key: &str, digest: Sha256Digest) {
        self.seen().insert(key.to_string(), digest);
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

    /// The guarded write every write of this surface ends in: the version of
    /// what it replaces kept first, the data folder's write lock held across
    /// the compare and the rename, and no ignore stamp, so the running app
    /// hears the write through its folder watcher as it hears any other
    /// program's (ADR-033).
    fn write_guarded(
        &self,
        file: &Path,
        bytes: &[u8],
        last_known: Option<DiskState>,
        on_disk: Option<DiskState>,
        on_conflict: ConflictPolicy,
        origin: WriteOrigin,
    ) -> Result<crate::guarded::WriteOutcome, StorageError> {
        if let Some(history) = self.history {
            // The capture after the write finds the note by its path, so a note
            // renamed since the store last saw it is moved to its new path
            // first, while the file still has the identity the store knows.
            if let Err(error) = history.follow_rename(file) {
                warn!(
                    note = %file_name_only(&file.to_string_lossy()),
                    error = %error,
                    "the versions this note kept under another name could not be moved to it"
                );
            }
        }
        let keep = self.history.map(keep_versions);
        write_note_guarded(
            GuardedWrite {
                target: file,
                bytes,
                last_known,
                // Read once by the caller, from the same bytes the digests it
                // passes describe.
                on_disk: DiskRead::Read(on_disk),
                dataless: None,
                origin,
                on_conflict,
                history: keep.as_ref().map(|hook| hook as &dyn Fn(WriteCapture<'_>)),
                lock: self.write_lock.as_ref(),
            },
            None,
        )
    }

    /// The digest a write is judged against, settled before the file is asked
    /// anything else, so a note this host has not seen is answered without
    /// being stat'ed for its download state or read.
    fn expected(
        &self,
        path: &str,
        key: &str,
        last_known: LastKnown,
    ) -> Result<Option<Sha256Digest>, HostError> {
        match last_known {
            LastKnown::Hash(hash) => Ok(Some(hash)),
            LastKnown::LastSeen => {
                self.seen()
                    .get(key)
                    .copied()
                    .map(Some)
                    .ok_or_else(|| HostError::HashRequired {
                        path: path.to_string(),
                    })
            }
            LastKnown::Overwrite => Ok(None),
        }
    }

    /// What a whole-file write the guard refuses leaves behind: the caller's
    /// text beside the note, or for a held write nothing, since the person
    /// applied a change to the text they were shown and not a new file.
    fn on_conflict(&self) -> ConflictPolicy {
        if self.review.is_some() {
            ConflictPolicy::RefuseOnly
        } else {
            ConflictPolicy::RefuseWithCopy
        }
    }

    /// Holds a write for the person, when this handle has a reviewer.
    ///
    /// `Ok(true)` once they applied it, `Ok(false)` for a handle that asks
    /// nobody, and the refusal for anything but Apply.
    fn reviewed(
        &self,
        path: &str,
        file: &Path,
        change: impl FnOnce() -> ProposedChange,
    ) -> Result<bool, HostError> {
        let Some(reviewer) = &self.review else {
            return Ok(false);
        };
        let proposal = WriteProposal {
            program: reviewer.program.clone(),
            path: file.to_string_lossy().into_owned(),
            change: change(),
        };
        match verdict(reviewer.review.review(&proposal)) {
            Ok(()) => Ok(true),
            Err(Refused::Discarded) => Err(HostError::Discarded {
                path: path.to_string(),
            }),
            Err(Refused::Unavailable(reason)) => Err(HostError::ReviewUnavailable {
                path: path.to_string(),
                reason,
            }),
        }
    }

    /// One try at [`NoteHost::append_note`]: `None` when the note changed
    /// between the read and the rename and is worth reading again.
    fn try_append(
        &self,
        path: &str,
        file: &Path,
        entry: &str,
        origin: &WriteOrigin,
    ) -> Result<Option<WriteReceipt>, HostError> {
        let bytes = std::fs::metadata(file)
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
        let read = std::fs::read(file).map_err(|_| HostError::Unreadable {
            path: path.to_string(),
        })?;
        let decoded =
            writ_core::text_encoding::decode_file(&read).ok_or_else(|| HostError::NotText {
                path: path.to_string(),
            })?;
        let appended = writ_core::capture::append_entry(&decoded.text, entry);
        let written = decoded
            .encoding
            .encode(&appended)
            .map_err(|lossy| HostError::EncodingLossy {
                path: path.to_string(),
                encoding: lossy.encoding.name().to_string(),
                characters: writ_core::text_encoding::code_points(&lossy.characters),
            })?
            .into_owned();
        // The append is compared against the read it was built from: the
        // entry goes after exactly those bytes or not at all.
        let state = DiskState {
            hash: writ_core::hash::sha256_bytes(&read),
            size: read.len() as u64,
            mtime: std::fs::metadata(file).ok().and_then(|m| m.modified().ok()),
        };
        let asked = self.reviewed(path, file, || ProposedChange::Edit {
            before: decoded.text.to_string(),
            after: appended.clone(),
        })?;
        match self.write_guarded(
            file,
            &written,
            Some(state),
            Some(state),
            ConflictPolicy::RefuseOnly,
            origin.clone(),
        ) {
            Ok(outcome) => {
                let key = notes_index::index_key(file);
                self.saw(&key, outcome.disk_state.hash);
                Ok(Some(WriteReceipt {
                    path: key,
                    bytes: outcome.disk_state.size,
                    hash: digest_hex(outcome.disk_state.hash),
                    changed: true,
                }))
            }
            // A change the person applied is the change they saw; built again
            // over a file that moved on, it would be one they did not.
            Err(StorageError::SourceChangedOnDisk { .. }) if asked => Err(HostError::Conflict {
                path: path.to_string(),
                conflict_copy: None,
            }),
            Err(StorageError::SourceChangedOnDisk { .. }) => Ok(None),
            Err(error) => Err(write_error(path, error)),
        }
    }

    /// One try at [`NoteHost::replace_in_note`]: `None` when the note changed
    /// between the read and the rename and is worth reading again.
    fn try_replace(
        &self,
        path: &str,
        file: &Path,
        old: &str,
        new: &str,
        origin: &WriteOrigin,
    ) -> Result<Option<PassageReceipt>, HostError> {
        let read = read_whole_note(path, file)?;
        let decoded =
            writ_core::text_encoding::decode_file(&read).ok_or_else(|| HostError::NotText {
                path: path.to_string(),
            })?;
        let replaced = replace_passage(&decoded.text, old, new).map_err(|error| match error {
            PassageError::Empty => HostError::PassageEmpty {
                path: path.to_string(),
            },
            PassageError::Absent => HostError::PassageAbsent {
                path: path.to_string(),
            },
            PassageError::Repeated { count } => HostError::PassageRepeated {
                path: path.to_string(),
                count,
            },
        })?;
        let written = decoded
            .encoding
            .encode(&replaced.text)
            .map_err(|lossy| HostError::EncodingLossy {
                path: path.to_string(),
                encoding: lossy.encoding.name().to_string(),
                characters: writ_core::text_encoding::code_points(&lossy.characters),
            })?
            .into_owned();
        // Compared against the read the replacement was made in: the new text
        // goes in place of exactly those bytes or not at all.
        let state = DiskState {
            hash: writ_core::hash::sha256_bytes(&read),
            size: read.len() as u64,
            mtime: std::fs::metadata(file).ok().and_then(|m| m.modified().ok()),
        };
        let asked = self.reviewed(path, file, || ProposedChange::Edit {
            before: decoded.text.to_string(),
            after: replaced.text.clone(),
        })?;
        match self.write_guarded(
            file,
            &written,
            Some(state),
            Some(state),
            ConflictPolicy::RefuseOnly,
            origin.clone(),
        ) {
            Ok(outcome) => {
                let key = notes_index::index_key(file);
                self.saw(&key, outcome.disk_state.hash);
                Ok(Some(PassageReceipt {
                    written: WriteReceipt {
                        path: key,
                        bytes: outcome.disk_state.size,
                        hash: digest_hex(outcome.disk_state.hash),
                        changed: outcome.decision
                            != writ_core::notes::guard::SaveDecision::AlreadyIdentical,
                    },
                    line: replaced.line,
                }))
            }
            // A change the person applied is the change they saw; built again
            // over a file that moved on, it would be one they did not.
            Err(StorageError::SourceChangedOnDisk { .. }) if asked => Err(HostError::Conflict {
                path: path.to_string(),
                conflict_copy: None,
            }),
            Err(StorageError::SourceChangedOnDisk { .. }) => Ok(None),
            Err(error) => Err(write_error(path, error)),
        }
    }
}

/// What the note at `file` holds, and the state a write is compared with,
/// or `None` when there is no file there.
fn read_held_note(path: &str, file: &Path) -> Result<Option<(DiskState, Vec<u8>)>, HostError> {
    if !file.exists() {
        return Ok(None);
    }
    let read = read_whole_note(path, file)?;
    let state = DiskState {
        hash: writ_core::hash::sha256_bytes(&read),
        size: read.len() as u64,
        mtime: std::fs::metadata(file).ok().and_then(|m| m.modified().ok()),
    };
    Ok(Some((state, read)))
}

/// A note's bytes as the person reads them, for a held write's comparison.
fn text_of(bytes: &[u8]) -> String {
    writ_core::text_encoding::decode_file(bytes)
        .map(|decoded| decoded.text.into_owned())
        .unwrap_or_else(|| String::from_utf8_lossy(bytes).into_owned())
}

/// The bytes of the note at `file`, held to [`MAX_NOTE_BYTES`].
fn read_whole_note(path: &str, file: &Path) -> Result<Vec<u8>, HostError> {
    let bytes = std::fs::metadata(file)
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
    std::fs::read(file).map_err(|_| HostError::Unreadable {
        path: path.to_string(),
    })
}

impl NoteHost for NoteHostImpl<'_> {
    fn list_notes(
        &self,
        prefix: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NoteSummary>, HostError> {
        self.permit(Capability::ListNotes)?;

        let mut notes = Vec::new();
        for entry in notes_index::build_notes_walk(&self.notes_root).build() {
            let Ok(entry) = entry else { continue };
            if !entry.file_type().is_some_and(|kind| kind.is_file()) {
                continue;
            }
            let path = entry.path();
            if writ_core::workspace::path_has_notes_ignored_name(&self.notes_root, path) {
                continue;
            }
            if !writ_core::notes::extensions::has_text_extension(path) {
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
        // Read as the open and the index read it (ADR-049). The digest is of
        // the file's bytes, which is what a write compares it with.
        let text = writ_core::text_encoding::decode_file(&read)
            .ok_or_else(|| HostError::NotText {
                path: path.to_string(),
            })?
            .text
            .into_owned();
        let key = notes_index::index_key(&file);
        let digest = writ_core::hash::sha256_bytes(&read);
        self.saw(&key, digest);
        Ok(NoteContent {
            path: key,
            bytes,
            hash: digest_hex(digest),
            text,
        })
    }

    fn heading_line(&self, path: &str, heading: &str) -> Result<Option<u32>, HostError> {
        self.permit(Capability::ReadNote)?;
        let file = self.note_file(path)?;
        let read = read_whole_note(path, &file)?;
        let text =
            writ_core::text_encoding::decode_file(&read).ok_or_else(|| HostError::NotText {
                path: path.to_string(),
            })?;
        Ok(writ_core::open_context::heading_line(&text.text, heading))
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

    fn search_notes(
        &self,
        query: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<NoteHit>, HostError> {
        self.permit(Capability::SearchNotes)?;
        let index = self.index()?;

        let Some(planned) = writ_core::search::plan_search(query) else {
            return Ok(Vec::new());
        };
        let hits = index
            .search(&planned, offset, limit)
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

    fn folder_index(&self) -> Result<Vec<IndexEntry>, HostError> {
        self.permit(Capability::ReadIndex)?;
        let index = self.index()?;
        let rows = index
            .folder_index()
            .map_err(|_| HostError::IndexUnavailable)?;
        Ok(rows
            .into_iter()
            .map(|row| IndexEntry {
                title: row
                    .title
                    .unwrap_or_else(|| writ_core::notes::note_display_name(&row.path)),
                path: row.path,
                bytes: row.bytes,
                tags: row.tags,
                links: row.links,
            })
            .collect())
    }

    fn write_note(
        &self,
        path: &str,
        content: &str,
        last_known: LastKnown,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::WriteNote)?;
        let file = self.note_file(path)?;
        let key = notes_index::index_key(&file);
        let expected = self.expected(path, &key, last_known)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5). The guard asks the same
        // question, and asking it here is what lets the state below be read
        // once and handed over rather than read again inside it.
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
        // A held write shows what the file holds, so the bytes are read once
        // and the state the guard compares is taken from them.
        let (on_disk, held) = if self.review.is_some() {
            match read_held_note(path, &file)? {
                Some((state, read)) => (Some(state), Some(read)),
                None => (None, None),
            }
        } else {
            let state = read_disk_state(&file).map_err(|_| HostError::Unreadable {
                path: path.to_string(),
            })?;
            (state, None)
        };
        // Asked only for a write that would land: one the guard is about to
        // refuse, or one that leaves the file as it is, holds nothing.
        let current = expected.is_none_or(|hash| on_disk.map(|state| state.hash) == Some(hash));
        let unchanged = held.as_deref() == Some(content.as_bytes());
        // Refused here rather than by the guard, which writes a missing file
        // and sets refused text beside the note: a held write reaches the
        // folder only as the person applied it.
        if self.review.is_some() && !current && !unchanged {
            return Err(HostError::Conflict {
                path: path.to_string(),
                conflict_copy: None,
            });
        }
        if current && !unchanged {
            self.reviewed(path, &file, || match &held {
                Some(read) => ProposedChange::Edit {
                    before: text_of(read),
                    after: content.to_string(),
                },
                None => ProposedChange::Create {
                    after: content.to_string(),
                },
            })?;
        }
        // With a digest, only that is compared: the guard reads neither the
        // length nor the modification time, and a caller that read the note
        // over a wire knows neither about the file it read. For an overwrite,
        // what the file holds now stands in, which is the same thing as having
        // no expectation and also lets a write of the text the note already
        // holds be recognised and skipped.
        let last_known = match expected {
            Some(hash) => Some(DiskState {
                hash,
                size: 0,
                mtime: None,
            }),
            None => on_disk,
        };
        let outcome = self
            .write_guarded(
                &file,
                content.as_bytes(),
                last_known,
                on_disk,
                self.on_conflict(),
                origin,
            )
            .map_err(|error| write_error(path, error))?;
        // Landed or already there, the note now holds the caller's text, which
        // is what the next write from this host is compared with.
        self.saw(&key, outcome.disk_state.hash);
        Ok(WriteReceipt {
            path: key,
            bytes: outcome.disk_state.size,
            hash: digest_hex(outcome.disk_state.hash),
            // The guard leaves a file that already holds the incoming bytes
            // alone. Reporting that as a write is what lets a caller tell
            // somebody their change landed when nothing happened.
            changed: outcome.decision != writ_core::notes::guard::SaveDecision::AlreadyIdentical,
        })
    }

    fn replace_note(
        &self,
        path: &str,
        content: &str,
        last_known: LastKnown,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::WriteNote)?;
        let file = self.note_file(path)?;
        let key = notes_index::index_key(&file);
        let expected = self.expected(path, &key, last_known)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5).
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
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
        let decoded =
            writ_core::text_encoding::decode_file(&read).ok_or_else(|| HostError::NotText {
                path: path.to_string(),
            })?;
        let shaped = writ_core::notes::line_ending::in_shape_of(&decoded.text, content);
        let written = decoded
            .encoding
            .encode(&shaped)
            .map_err(|lossy| HostError::EncodingLossy {
                path: path.to_string(),
                encoding: lossy.encoding.name().to_string(),
                characters: writ_core::text_encoding::code_points(&lossy.characters),
            })?
            .into_owned();
        let on_disk = DiskState {
            hash: writ_core::hash::sha256_bytes(&read),
            size: read.len() as u64,
            mtime: std::fs::metadata(&file)
                .ok()
                .and_then(|m| m.modified().ok()),
        };
        if expected.is_none_or(|hash| hash == on_disk.hash) && read != written {
            self.reviewed(path, &file, || ProposedChange::Edit {
                before: decoded.text.to_string(),
                after: shaped.to_string(),
            })?;
        }
        // With a digest, only that is compared, as `write_note` compares it.
        let last_known = match expected {
            Some(hash) => DiskState {
                hash,
                size: 0,
                mtime: None,
            },
            None => on_disk,
        };
        let outcome = self
            .write_guarded(
                &file,
                &written,
                Some(last_known),
                Some(on_disk),
                self.on_conflict(),
                origin,
            )
            .map_err(|error| write_error(path, error))?;
        self.saw(&key, outcome.disk_state.hash);
        Ok(WriteReceipt {
            path: key,
            bytes: outcome.disk_state.size,
            hash: digest_hex(outcome.disk_state.hash),
            changed: outcome.decision != writ_core::notes::guard::SaveDecision::AlreadyIdentical,
        })
    }

    fn append_note(
        &self,
        path: &str,
        text: &str,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::WriteNote)?;
        let file = self.note_file(path)?;
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5).
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
        let Some(entry) = writ_core::capture::capture_entry(text) else {
            let state =
                read_disk_state(&file)
                    .ok()
                    .flatten()
                    .ok_or_else(|| HostError::Unreadable {
                        path: path.to_string(),
                    })?;
            return Ok(WriteReceipt {
                path: notes_index::index_key(&file),
                bytes: state.size,
                hash: digest_hex(state.hash),
                changed: false,
            });
        };
        for _ in 0..APPEND_ATTEMPTS {
            if let Some(receipt) = self.try_append(path, &file, &entry, &origin)? {
                return Ok(receipt);
            }
        }
        Err(HostError::Conflict {
            path: path.to_string(),
            conflict_copy: None,
        })
    }

    fn replace_in_note(
        &self,
        path: &str,
        old: &str,
        new: &str,
        origin: WriteOrigin,
    ) -> Result<PassageReceipt, HostError> {
        self.permit(Capability::WriteNote)?;
        let file = self.note_file(path)?;
        if old.is_empty() {
            return Err(HostError::PassageEmpty {
                path: path.to_string(),
            });
        }
        // Asked before the read below, because the read is what would pull an
        // evicted file down (ADR-028 section 5).
        if is_not_downloaded(dataless_flags(&file)) {
            return Err(HostError::NotDownloaded {
                path: path.to_string(),
            });
        }
        for _ in 0..APPEND_ATTEMPTS {
            if let Some(receipt) = self.try_replace(path, &file, old, new, &origin)? {
                return Ok(receipt);
            }
        }
        Err(HostError::Conflict {
            path: path.to_string(),
            conflict_copy: None,
        })
    }

    fn create_note(
        &self,
        name: &str,
        content: &str,
        origin: WriteOrigin,
    ) -> Result<WriteReceipt, HostError> {
        self.permit(Capability::CreateNote)?;
        // A name that spells a text format keeps it, as `writ new` and the
        // window's own New File do (ADR-041 §2): `Plan.md` is not `Plan.md.txt`.
        let (stem, extension) =
            writ_core::notes::requested_note_name(name, self.default_extension.as_str())
                .ok_or(HostError::NameEmpty)?;
        self.reviewed(
            name,
            &self.notes_root.join(format!("{stem}.{extension}")),
            || ProposedChange::Create {
                after: content.to_string(),
            },
        )?;
        // The dedupe is what a person who asked for a new note wants and the
        // wrong answer for a program: a caller that asked for `Launch` and got
        // `Launch 2` has put its text in a note it did not name. The facade
        // holds the folding rule for what "taken" means, so it is asked rather
        // than second-guessed: a check here against the exact path would be the
        // filesystem's answer, and a case-sensitive volume folds nothing.
        let minted = create_note_guarded(
            CreateNote {
                notes_root: &self.notes_root,
                stem: &stem,
                extension,
                content,
                origin,
                on_taken_name: TakenName::Refuse,
                // A note that did not exist has no earlier text to keep.
                history: None,
                lock: self.write_lock.as_ref(),
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
        let key = notes_index::index_key(&minted);
        self.saw(&key, state.hash);
        Ok(WriteReceipt {
            path: key,
            bytes: state.size,
            hash: digest_hex(state.hash),
            // A note that did not exist a moment ago holds new bytes.
            changed: true,
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
        self.reviewed(path, &file, || ProposedChange::Rename {
            to: match file.extension() {
                Some(extension) => format!("{stem}.{}", extension.to_string_lossy()),
                None => stem.to_string(),
            },
        })?;
        let moved = crate::note_ops::rename_note(
            &file, &stem, last_known, origin,
            // No ignore stamp, for the reason `write_note` gives.
            None,
        )
        .map_err(|error| write_error(path, error))?;
        let new_key = notes_index::index_key(&moved);
        let previous_key = notes_index::index_key(&file);
        // A rename moves no byte, so what was seen at the old path is what the
        // note holds at the new one.
        {
            let mut seen = self.seen();
            if let Some(digest) = seen.remove(&previous_key) {
                seen.insert(new_key.clone(), digest);
            }
        }
        Ok(RenameReceipt {
            path: new_key,
            previous_path: previous_key,
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
