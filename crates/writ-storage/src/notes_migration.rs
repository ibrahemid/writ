//! The one-time pass that turns every note into a file (ADR-028 §4).
//!
//! Before 0.4 a note's text lived in a mirror under `~/.writ/buffers/`, and
//! the file it came from — when it had one — was a secondary artefact. This
//! pass reverses that: every row ends up either pointing at a file that holds
//! its text, written into the archive with its text, deleted because it held
//! none, or listed as a failure with its mirror still in place.
//!
//! Mechanism only. Every naming and dedupe decision comes from
//! [`writ_core::notes`], so the migration, a rename and a new note all mint
//! the same name from the same title.
//!
//! Two rules make the pass safe to run against a real database. Nothing is
//! deleted before the file that replaces it has been read back and compared
//! by SHA-256, and the whole database is copied to
//! `writ.db.pre-notes-migration` before the first write ([`crate::rollback`]).
//! Neither undoes a run that mis-named a file; together they bound what a run
//! can destroy to nothing.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tracing::warn;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::hash::sha256_bytes;
use writ_core::notes;

use crate::atomic::write_atomic;
use crate::buffer_store::BufferStore;
use crate::database::queries;
use crate::errors::StorageResult;
use crate::rollback;
use crate::schema_meta::{self, KEY_NOTES_MIGRATION_RAN_AT, KEY_NOTES_MIGRATION_REPORT};

/// Folder inside the notes folder that holds text the migration could not
/// place with confidence.
pub const RECOVERED_FOLDER: &str = "Recovered";

/// What happened to one row, or to one file of piped input.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RowOutcome {
    /// Already had a file, and the mirror agreed with it; mirror unlinked.
    AlreadyOnDisk {
        /// The file the note lives in.
        path: String,
    },
    /// The mirror held edits the file never received; written beside the note.
    RecoveredUnsavedEdits {
        /// The file the note lives in, left byte-identical.
        source: String,
        /// Where the mirror's text was written.
        recovered: String,
    },
    /// An active row with no file, written into the notes folder.
    WrittenToNotes {
        /// The file the note now lives in.
        path: String,
    },
    /// A history row with no file, written into the archive folder.
    Archived {
        /// Where the text was written.
        path: String,
    },
    /// The file the row named was gone or unreadable, so the mirror's text was
    /// written into `Recovered/` and the row repointed at it.
    RecoveredMissingFile {
        /// The file the row used to name.
        source: String,
        /// The file the note now lives in.
        path: String,
    },
    /// A file of piped input, moved into the notes folder.
    PipedFile {
        /// Where the piped input was.
        from: String,
        /// The file it became.
        path: String,
    },
    /// The written file did not read back as what was written, or could not be
    /// written at all; the mirror was kept and nothing was deleted.
    VerificationFailed {
        /// Where the write was attempted.
        path: String,
        /// The mirror that was kept.
        mirror: String,
        /// Where the text landed instead, when `Recovered/` would take it.
        recovered: Option<String>,
    },
    /// The row held nothing and had no file; the row was deleted.
    DeletedEmpty,
    /// The row was already migrated and its file is still there.
    Skipped {
        /// The file the note lives in.
        path: String,
    },
}

/// One run's outcome, stored as JSON in [`crate::schema_meta`] and rendered by
/// the report panel.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MigrationReport {
    /// When the latest run finished, RFC 3339.
    pub ran_at: String,
    /// When the run that placed the first of these notes finished, RFC 3339.
    ///
    /// A re-run only revisits the rows that still need work, so it inherits
    /// everything an earlier one placed ([`Self::merge_over`]). This is the
    /// date those notes moved, which is the one the report has any reason to
    /// name.
    #[serde(default)]
    pub first_ran_at: String,
    /// The notes folder the run wrote into.
    pub notes_folder: String,
    /// The archive folder the run wrote history rows into.
    pub archive_folder: String,
    /// Notes that are now files in the notes folder.
    pub migrated: usize,
    /// Notes written into the archive folder.
    pub archived: usize,
    /// Notes whose text could only be placed under `Recovered/`: edits a file
    /// never received, or a file that was gone by the time the pass ran.
    pub recovered: usize,
    /// Rows whose written file did not verify.
    pub failed: usize,
    /// Rows that held nothing and were deleted.
    pub deleted_empty: usize,
    /// Files of piped input moved into the notes folder.
    pub piped: usize,
    /// Every row, keyed by its id, or by the file name for piped input that
    /// no row owns.
    pub rows: Vec<(String, RowOutcome)>,
}

impl MigrationReport {
    /// Replaces the outcome recorded for `key`, or appends it.
    ///
    /// A row can be visited twice — once for its own text and once because a
    /// file of piped input turned out to be the file it points at — and the
    /// second visit is what actually happened to it.
    fn record(&mut self, key: String, outcome: RowOutcome) {
        match self.rows.iter_mut().find(|(existing, _)| *existing == key) {
            Some(entry) => entry.1 = outcome,
            None => self.rows.push((key, outcome)),
        }
    }

    /// Folds `previous` under this run's outcomes.
    ///
    /// A re-run visits every row but only does work on the ones that still
    /// need it; the rest come back [`RowOutcome::Skipped`], which says where a
    /// note is but not what became of it. Storing those over the last run's
    /// answers would tell a user with forty notes that one was migrated, so a
    /// row keeps the last outcome that said something and only a real outcome
    /// replaces it.
    fn merge_over(&mut self, previous: MigrationReport) {
        self.first_ran_at = if previous.first_ran_at.is_empty() {
            previous.ran_at
        } else {
            previous.first_ran_at
        };
        let mut rows = previous.rows;
        for (key, outcome) in self.rows.drain(..) {
            match rows.iter_mut().find(|(existing, _)| *existing == key) {
                Some(entry) => {
                    if !matches!(outcome, RowOutcome::Skipped { .. }) {
                        entry.1 = outcome;
                    }
                }
                None => rows.push((key, outcome)),
            }
        }
        self.rows = rows;
    }

    /// Recounts every total from [`Self::rows`].
    fn recount(&mut self) {
        let (mut migrated, mut archived, mut recovered) = (0, 0, 0);
        let (mut failed, mut deleted_empty, mut piped) = (0, 0, 0);
        for (_, outcome) in &self.rows {
            match outcome {
                RowOutcome::AlreadyOnDisk { .. } | RowOutcome::WrittenToNotes { .. } => {
                    migrated += 1
                }
                RowOutcome::Archived { .. } => archived += 1,
                RowOutcome::RecoveredUnsavedEdits { .. }
                | RowOutcome::RecoveredMissingFile { .. } => recovered += 1,
                RowOutcome::PipedFile { .. } => piped += 1,
                RowOutcome::VerificationFailed { .. } => failed += 1,
                RowOutcome::DeletedEmpty => deleted_empty += 1,
                RowOutcome::Skipped { .. } => {}
            }
        }
        self.migrated = migrated;
        self.archived = archived;
        self.recovered = recovered;
        self.failed = failed;
        self.deleted_empty = deleted_empty;
        self.piped = piped;
    }
}

/// The folders a run writes into.
///
/// `piped` is `~/.writ/piped/`, where the CLI wrote piped input before 0.4.
/// Its files are notes nobody named, so they migrate on the same pass as rows
/// with no file, and the folder is left empty (ADR-028 §1).
#[derive(Debug, Clone, Copy)]
pub struct MigrationRoots<'a> {
    /// The database the rollback copy is taken from.
    pub db_path: &'a Path,
    /// Where active notes land.
    pub notes: &'a Path,
    /// Where history notes land, under Writ's own data folder.
    pub archive: &'a Path,
    /// Where the CLI wrote piped input before 0.4.
    pub piped: &'a Path,
}

/// Runs the one-time notes migration.
///
/// Idempotent: a run that left no copy behind returns the report the earlier
/// run stored and touches nothing, whatever became of the files afterwards
/// ([`is_settled`]). Otherwise the pass runs again, skipping each row that
/// already has its file.
///
/// The rollback copy is written immediately before the first change, and only
/// when there is one to make, so a launch with nothing to migrate leaves no
/// copy behind. Every file written is read back and compared by SHA-256
/// against the bytes it came from, and a mirror is unlinked only after its
/// comparison passes.
pub fn run_notes_migration(
    store: &BufferStore,
    roots: MigrationRoots<'_>,
    now: DateTime<Utc>,
) -> StorageResult<MigrationReport> {
    let conn = store.connection();
    let rows = all_rows(store)?;
    let piped_files = list_piped_files(roots.piped);

    if is_settled(store, &rows, &piped_files)? {
        return Ok(stored_report(store)?.unwrap_or_default());
    }

    let mut report = MigrationReport {
        ran_at: now.to_rfc3339(),
        first_ran_at: now.to_rfc3339(),
        notes_folder: roots.notes.to_string_lossy().into_owned(),
        archive_folder: roots.archive.to_string_lossy().into_owned(),
        ..MigrationReport::default()
    };

    if !has_work(store, &rows, &piped_files)? {
        finish(store, &mut report)?;
        return Ok(report);
    }

    rollback::write_rollback_copy(conn, roots.db_path)?;

    let mut dirs = Destinations {
        notes: DirNames::read(roots.notes),
        archive: DirNames::read(roots.archive),
        recovered: DirNames::read(&roots.notes.join(RECOVERED_FOLDER)),
    };

    for doc in &rows {
        let outcome = migrate_row(store, doc, &mut dirs, now)?;
        report.record(doc.id.clone(), outcome);
    }

    migrate_piped_files(store, &rows, &piped_files, &mut dirs, now, &mut report)?;

    finish(store, &mut report)?;
    Ok(report)
}

/// The report the last run stored, when one parses.
pub fn stored_report(store: &BufferStore) -> StorageResult<Option<MigrationReport>> {
    let Some(json) = schema_meta::get(store.connection(), KEY_NOTES_MIGRATION_REPORT)? else {
        return Ok(None);
    };
    match serde_json::from_str(&json) {
        Ok(report) => Ok(Some(report)),
        Err(error) => {
            warn!(%error, "the stored notes-migration report could not be read");
            Ok(None)
        }
    }
}

/// Whether the migration has run and left nothing behind.
///
/// Two things earn a re-run, and the pass is idempotent so each costs only the
/// rows that still need it (ADR-028 §4 step 6). A row still has a copy under
/// the retired folder, which is what a failed verification leaves: retrying is
/// how that note stops reading as empty once whatever blocked the write is
/// gone, and it is the only state in which text exists somewhere the invariant
/// does not allow. Or piped input has arrived from a CLI that has not been
/// updated yet.
///
/// A file the pass placed and the user then deleted is not one of them. The
/// user deleting a note is the note being deleted; re-running the pass over it
/// would write the text back from a copy Writ no longer keeps, or, when the
/// copy is already gone, do nothing but re-stamp the run and lose the report
/// the first one left.
fn is_settled(
    store: &BufferStore,
    rows: &[BufferDocument],
    piped_files: &[PathBuf],
) -> StorageResult<bool> {
    if schema_meta::get(store.connection(), KEY_NOTES_MIGRATION_RAN_AT)?.is_none() {
        return Ok(false);
    }
    if !piped_files.is_empty() {
        return Ok(false);
    }
    Ok(!any_mirror_left(store, rows))
}

/// Whether any row still has a copy under the retired mirror folder.
fn any_mirror_left(store: &BufferStore, rows: &[BufferDocument]) -> bool {
    rows.iter()
        .any(|doc| store.buffers_dir().join(&doc.filename).exists())
}

/// Whether the pass has anything to change.
///
/// Answered before the rollback copy is taken, so a launch that would write
/// nothing leaves no copy of the database beside it. Cheap: one `metadata`
/// call per row at worst, and it stops at the first row that needs work.
fn has_work(
    store: &BufferStore,
    rows: &[BufferDocument],
    piped_files: &[PathBuf],
) -> StorageResult<bool> {
    if !piped_files.is_empty() {
        return Ok(true);
    }
    if any_mirror_left(store, rows) {
        return Ok(true);
    }
    let conn = store.connection();
    for doc in rows {
        let migrated = queries::get_migrated_path(conn, &doc.id)?;
        if migrated.is_some_and(|path| Path::new(&path).exists()) {
            continue;
        }
        return Ok(true);
    }
    Ok(false)
}

/// Stamps the run and stores its report, folded over the one the last run
/// left behind ([`MigrationReport::merge_over`]).
fn finish(store: &BufferStore, report: &mut MigrationReport) -> StorageResult<()> {
    if let Some(previous) = stored_report(store)? {
        report.merge_over(previous);
    }
    report.recount();
    let conn = store.connection();
    let json = serde_json::to_string(&report)?;
    schema_meta::set(conn, KEY_NOTES_MIGRATION_REPORT, &json)?;
    schema_meta::set(conn, KEY_NOTES_MIGRATION_RAN_AT, &report.ran_at)?;
    Ok(())
}

fn all_rows(store: &BufferStore) -> StorageResult<Vec<BufferDocument>> {
    let mut rows = store.list_by_status(BufferStatus::Active)?;
    rows.extend(store.list_by_status(BufferStatus::History)?);
    Ok(rows)
}

/// Migrates one row (ADR-028 §4 steps 2 to 4).
fn migrate_row(
    store: &BufferStore,
    doc: &BufferDocument,
    dirs: &mut Destinations,
    now: DateTime<Utc>,
) -> StorageResult<RowOutcome> {
    let conn = store.connection();
    let mirror = store.buffers_dir().join(&doc.filename);

    if let Some(path) = queries::get_migrated_path(conn, &doc.id)? {
        if Path::new(&path).exists() {
            // The copy outlived the run that placed this note, which the
            // settle check reads as unfinished work. It is safe to clear: the
            // file it was compared against is still there.
            remove_mirror(&mirror);
            return Ok(RowOutcome::Skipped { path });
        }
    }

    let mirror_bytes = std::fs::read(&mirror).ok();

    match doc.source_path.as_deref() {
        Some(source_path) => {
            migrate_source_backed_row(store, doc, source_path, &mirror, mirror_bytes, dirs, now)
        }
        None => migrate_row_without_a_file(store, doc, &mirror, mirror_bytes, dirs, now),
    }
}

/// A row that already names a file (ADR-028 §4 step 2).
fn migrate_source_backed_row(
    store: &BufferStore,
    doc: &BufferDocument,
    source_path: &str,
    mirror: &Path,
    mirror_bytes: Option<Vec<u8>>,
    dirs: &mut Destinations,
    now: DateTime<Utc>,
) -> StorageResult<RowOutcome> {
    let conn = store.connection();
    let source_bytes = std::fs::read(source_path).ok();

    let Some(mirror_bytes) = mirror_bytes else {
        // Nothing was ever copied, or a previous run already cleared it. The
        // file is the only copy either way, which is where the row belongs.
        queries::mark_migrated(conn, &doc.id, source_path, now.timestamp())?;
        return Ok(RowOutcome::AlreadyOnDisk {
            path: source_path.to_string(),
        });
    };

    if let Some(source_bytes) = source_bytes {
        if sha256_bytes(&mirror_bytes) == sha256_bytes(&source_bytes) {
            queries::mark_migrated(conn, &doc.id, source_path, now.timestamp())?;
            remove_mirror(mirror);
            return Ok(RowOutcome::AlreadyOnDisk {
                path: source_path.to_string(),
            });
        }

        // The copy holds text the file never received, which is what the save
        // defect in 0.3.0 through 0.3.2 produced. The file is left exactly as
        // it is and the copy is written beside it under its own name.
        let stem = recovered_stem(doc, source_path, now);
        let destination = dirs.recovered.allocate(&stem, extension_for(&mirror_bytes));
        return Ok(
            match write_and_verify(&destination, &mirror_bytes, &dirs.recovered) {
                Ok(()) => {
                    queries::mark_migrated(conn, &doc.id, source_path, now.timestamp())?;
                    remove_mirror(mirror);
                    RowOutcome::RecoveredUnsavedEdits {
                        source: source_path.to_string(),
                        recovered: path_text(&destination),
                    }
                }
                Err(error) => {
                    warn!(path = %destination.display(), %error, "could not write the recovered text");
                    RowOutcome::VerificationFailed {
                        path: path_text(&destination),
                        mirror: path_text(mirror),
                        recovered: recover_after_failure(&mut dirs.recovered, &stem, &mirror_bytes),
                    }
                }
            },
        );
    }

    // The file is gone or unreadable, so the copy is the only text left. It
    // goes into Recovered/ rather than the notes folder, because nobody chose
    // the name it would land under.
    let stem = recovered_stem(doc, source_path, now);
    let destination = dirs.recovered.allocate(&stem, extension_for(&mirror_bytes));
    match write_and_verify(&destination, &mirror_bytes, &dirs.recovered) {
        Ok(()) => {
            let text = path_text(&destination);
            store.update_source_path(&doc.id, &text)?;
            queries::mark_migrated(conn, &doc.id, &text, now.timestamp())?;
            remove_mirror(mirror);
            Ok(RowOutcome::RecoveredMissingFile {
                source: source_path.to_string(),
                path: text,
            })
        }
        Err(error) => {
            warn!(path = %destination.display(), %error, "could not write the recovered text");
            Ok(RowOutcome::VerificationFailed {
                path: path_text(&destination),
                mirror: path_text(mirror),
                recovered: recover_after_failure(&mut dirs.recovered, &stem, &mirror_bytes),
            })
        }
    }
}

/// A row that names no file (ADR-028 §4 step 3).
fn migrate_row_without_a_file(
    store: &BufferStore,
    doc: &BufferDocument,
    mirror: &Path,
    mirror_bytes: Option<Vec<u8>>,
    dirs: &mut Destinations,
    now: DateTime<Utc>,
) -> StorageResult<RowOutcome> {
    let conn = store.connection();
    let bytes = mirror_bytes.unwrap_or_default();

    if bytes.is_empty() {
        // Nothing anywhere: no file, no text. Deleting the row is what keeps
        // the archive from filling with blank files.
        store.delete(&doc.id)?;
        return Ok(RowOutcome::DeletedEmpty);
    }

    let stem = note_stem(doc, now);
    let extension = extension_for(&bytes);
    let is_active = doc.status == BufferStatus::Active;
    let directory = if is_active {
        &mut dirs.notes
    } else {
        &mut dirs.archive
    };
    let destination = directory.allocate(&stem, extension);

    match write_and_verify(&destination, &bytes, directory) {
        Ok(()) => {
            let text = path_text(&destination);
            if is_active {
                store.update_source_path(&doc.id, &text)?;
            }
            queries::mark_migrated(conn, &doc.id, &text, now.timestamp())?;
            remove_mirror(mirror);
            Ok(if is_active {
                RowOutcome::WrittenToNotes { path: text }
            } else {
                // A history row keeps a NULL source_path: its file sits under
                // Writ's own data folder, and writing a hundred closed tabs
                // into a folder that may be syncing is the user's call to
                // make, not the migration's (ADR-028 §4 step 3).
                RowOutcome::Archived { path: text }
            })
        }
        Err(error) => {
            warn!(path = %destination.display(), %error, "could not write the note");
            Ok(RowOutcome::VerificationFailed {
                path: path_text(&destination),
                mirror: path_text(mirror),
                recovered: recover_after_failure(&mut dirs.recovered, &stem, &bytes),
            })
        }
    }
}

/// Moves every file of piped input into the notes folder.
///
/// A file here may already be the file a row points at: the CLI wrote it and
/// then asked Writ to open it. Moving it therefore has to repoint that row,
/// or the note it opened becomes unopenable.
fn migrate_piped_files(
    store: &BufferStore,
    rows: &[BufferDocument],
    piped_files: &[PathBuf],
    dirs: &mut Destinations,
    now: DateTime<Utc>,
    report: &mut MigrationReport,
) -> StorageResult<()> {
    if piped_files.is_empty() {
        return Ok(());
    }
    let owners = owners_by_path(rows);
    let conn = store.connection();

    for file in piped_files {
        let Ok(bytes) = std::fs::read(file) else {
            continue;
        };
        let owner = resolve_key(file).and_then(|key| owners.get(&key)).cloned();

        if bytes.is_empty() {
            remove_mirror(file);
            continue;
        }

        let stem = piped_stem(file, now);
        let destination = dirs.notes.allocate(&stem, extension_for(&bytes));
        let key = owner.clone().unwrap_or_else(|| file_name(file));

        match write_and_verify(&destination, &bytes, &dirs.notes) {
            Ok(()) => {
                let text = path_text(&destination);
                if let Some(id) = &owner {
                    store.update_source_path(id, &text)?;
                    queries::mark_migrated(conn, id, &text, now.timestamp())?;
                }
                remove_mirror(file);
                report.record(
                    key,
                    RowOutcome::PipedFile {
                        from: path_text(file),
                        path: text,
                    },
                );
            }
            Err(error) => {
                warn!(path = %destination.display(), %error, "could not write the piped text");
                report.record(
                    key,
                    RowOutcome::VerificationFailed {
                        path: path_text(&destination),
                        mirror: path_text(file),
                        recovered: recover_after_failure(&mut dirs.recovered, &stem, &bytes),
                    },
                );
            }
        }
    }
    Ok(())
}

/// Every regular file directly inside `piped`, sorted so a run is repeatable.
fn list_piped_files(piped: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(piped) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .filter(|path| !file_name(path).starts_with('.'))
        .collect();
    files.sort();
    files
}

/// Maps the file each row points at to the row's id, so a moved file can find
/// the note that opened it.
fn owners_by_path(rows: &[BufferDocument]) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for doc in rows {
        let Some(source_path) = doc.source_path.as_deref() else {
            continue;
        };
        if let Some(key) = resolve_key(Path::new(source_path)) {
            map.insert(key, doc.id.clone());
        }
    }
    map
}

/// A path in the one form two spellings of it agree on. `None` when the path
/// does not resolve, which means no row can be pointing at it.
fn resolve_key(path: &Path) -> Option<String> {
    std::fs::canonicalize(path)
        .ok()
        .map(|resolved| resolved.to_string_lossy().into_owned())
}

/// The stem a row with no file earns.
///
/// A title equal to the row's `filename` is a legacy row whose title was the
/// mirror's name, which names nothing; it is dated like the minted titles.
fn note_stem(doc: &BufferDocument, now: DateTime<Utc>) -> String {
    let dated_from = if doc.created_at.timestamp() > 0 {
        doc.created_at
    } else {
        now
    };
    let title = if doc.title == doc.filename {
        ""
    } else {
        doc.title.as_str()
    };
    notes::note_file_stem(title, dated_from)
}

/// The stem the text of a diverged or orphaned row is written under.
fn recovered_stem(doc: &BufferDocument, source_path: &str, now: DateTime<Utc>) -> String {
    let file_stem = Path::new(source_path)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    let named = if file_stem.trim().is_empty() {
        note_stem(doc, now)
    } else {
        notes::sanitize_title_or(&file_stem, &notes::date_stem(now))
    };
    let day = notes::date_stem(now);
    notes::sanitize_title_or(&format!("{named} (unsaved edits {day})"), &named)
}

/// The stem a file of piped input earns from its own name.
fn piped_stem(file: &Path, now: DateTime<Utc>) -> String {
    let stem = file
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or_default();
    notes::note_file_stem(&stem, now)
}

/// `md` for text, `txt` for anything that is not valid UTF-8. A file that is
/// not text is not a note, and naming it `.md` would tell the preview to try
/// to render it.
fn extension_for(bytes: &[u8]) -> &'static str {
    if std::str::from_utf8(bytes).is_ok() {
        "md"
    } else {
        "txt"
    }
}

/// Writes `bytes` to `destination` and reads them back, comparing SHA-256.
///
/// The read-back is the whole point: a write that reported success and landed
/// as something else is the one failure that would cost a note, because the
/// mirror is unlinked on the strength of this answer (ADR-028 §4 step 4).
fn write_and_verify(destination: &Path, bytes: &[u8], directory: &DirNames) -> std::io::Result<()> {
    directory.ensure()?;
    write_atomic(destination, bytes)?;
    let written = std::fs::read(destination)?;
    if sha256_bytes(&written) != sha256_bytes(bytes) {
        return Err(std::io::Error::other(
            "the file read back as different bytes",
        ));
    }
    Ok(())
}

/// Writes `bytes` into `Recovered/` after the write they were meant for
/// failed, returning where they landed.
///
/// A write that could not be made is not a reason for a note's text to exist
/// nowhere but a copy under a folder 0.4 retires. The copy is kept either way
/// (ADR-028 §4 step 4), so this only adds a file the user can find; `None`
/// when `Recovered/` will not take it either, which is what an unwritable
/// notes folder looks like.
fn recover_after_failure(recovered_dir: &mut DirNames, stem: &str, bytes: &[u8]) -> Option<String> {
    let destination = recovered_dir.allocate(stem, extension_for(bytes));
    match write_and_verify(&destination, bytes, recovered_dir) {
        Ok(()) => Some(path_text(&destination)),
        Err(error) => {
            warn!(path = %destination.display(), %error, "could not place the text under Recovered either");
            None
        }
    }
}

/// Clears a mirror, best-effort. A mirror that outlives the pass is reported
/// by the startup consistency check and costs nothing but space.
fn remove_mirror(mirror: &Path) {
    if mirror.exists() {
        if let Err(error) = std::fs::remove_file(mirror) {
            warn!(path = %mirror.display(), %error, "could not clear the copy");
        }
    }
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The three folders a run writes into, each carrying the names already taken
/// in it so nothing written overwrites anything already there.
struct Destinations {
    notes: DirNames,
    archive: DirNames,
    recovered: DirNames,
}

/// The names already taken in one folder, so the next one deduped against it
/// is Finder's answer rather than an overwrite.
///
/// The folder is created lazily: a run that writes nothing into `Recovered/`
/// or the archive leaves neither behind.
struct DirNames {
    dir: PathBuf,
    taken: HashSet<String>,
}

impl DirNames {
    fn read(dir: &Path) -> Self {
        let mut taken = HashSet::new();
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.filter_map(Result::ok) {
                taken.insert(entry.file_name().to_string_lossy().to_lowercase());
            }
        }
        Self {
            dir: dir.to_path_buf(),
            taken,
        }
    }

    /// Claims a deduped name and returns the path it resolves to. The name is
    /// claimed whether or not the write that follows succeeds, so a failed
    /// write never hands the same name to the next row.
    fn allocate(&mut self, stem: &str, extension: &str) -> PathBuf {
        let name = notes::dedupe_file_name(stem, extension, &self.taken);
        self.taken.insert(name.to_lowercase());
        self.dir.join(name)
    }

    fn ensure(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_follows_whether_the_bytes_are_text() {
        assert_eq!(extension_for(b"# notes"), "md");
        assert_eq!(extension_for(&[0xff, 0xfe, 0x00]), "txt");
    }

    #[test]
    fn a_claimed_name_is_never_handed_out_twice() {
        let dir = tempfile::tempdir().unwrap();
        let mut names = DirNames::read(dir.path());
        assert_eq!(names.allocate("Notes", "md"), dir.path().join("Notes.md"));
        assert_eq!(names.allocate("Notes", "md"), dir.path().join("Notes 2.md"));
        assert_eq!(names.allocate("notes", "md"), dir.path().join("notes 3.md"));
    }
}
