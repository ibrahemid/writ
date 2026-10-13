use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Maximum number of session markers retained per database.
///
/// A marker records whether a session is running or ended cleanly and holds
/// no text (ADR-044). Older markers are pruned on each write so the table
/// stays bounded.
pub const MAX_SNAPSHOTS: usize = 5;

/// How long the shutdown path waits for the frontend to confirm it flushed
/// before exiting anyway. A quit that hangs on an unresponsive webview is a
/// worse failure than a quit that loses the last debounce window.
pub const QUIT_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2000);

/// Whether the shutdown path should stop waiting and exit.
pub fn should_force_exit(waited: std::time::Duration, flush_confirmed: bool) -> bool {
    flush_confirmed || waited >= QUIT_FLUSH_TIMEOUT
}

/// A single note whose text the launch took from the unsaved-text journal.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredBuffer {
    /// Buffer id.
    pub id: String,
    /// The text the journal held for it.
    pub content: String,
    /// Nothing was written, so this content is the last copy of it: the note's
    /// file was deleted, or the launch could not give a note with no file its
    /// first one. Set by the launch, not by the journal.
    #[serde(default)]
    pub removed_on_disk: bool,
    /// The first file the launch could not make for a note that never had
    /// one, which the tab names in place of a deleted file. Set with
    /// `removed_on_disk`, and `None` for a note whose file was deleted.
    #[serde(default)]
    pub unmade_file: Option<UnmadeFile>,
}

/// The first file a launch could not make for a note: its name, and the
/// folder it was to be made in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnmadeFile {
    /// The file's name, as the note's first save would have given it.
    pub name: String,
    /// The name of the folder it was to be made in.
    pub folder: String,
}

/// What a relaunch does with the text the journal kept for a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecoveryPlan {
    /// Write it into the note's file.
    Write,
    /// Leave the path alone. The text goes to the tab instead, which comes up
    /// removed on disk with the ways out of that state.
    Withhold,
}

/// Decides whether a relaunch may write a note's recovered text to its path.
///
/// A note that never had a file has one minted for it at the relaunch, and the
/// journal is the only place its text is, so the write is what saves it. A
/// note that had a file and no longer has one is a file somebody deleted, and
/// writing it back at the next launch puts back what was thrown away, on every
/// device in a synced folder. That is the harm ADR-033 decision 14 exists to
/// avoid, and a relaunch is no better a moment for it than a save is. Its text
/// is handed to the tab instead (decision 15).
pub fn plan_recovery(had_file: bool, file_is_there: bool) -> RecoveryPlan {
    if had_file && !file_is_there {
        RecoveryPlan::Withhold
    } else {
        RecoveryPlan::Write
    }
}

/// Where a launch writes journaled text whose note's row has been deleted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanPlan {
    /// Beside the file the entry names, as a dated copy.
    BesideItsFile,
    /// In the notes folder, as a dated copy named for the file the entry
    /// names, or for the day when it names none.
    InNotesFolder,
}

/// Decides where a launch writes text whose note's row was deleted
/// ("Remove from history", "Clear history").
///
/// The entry keeps its own lifetime, so the text still reaches a file: there
/// is no row left to hand it to a tab. A dated copy goes beside the file the
/// entry names while that file is there, which is where the person will look.
/// A file that is gone was deleted, and a copy in the folder somebody cleared
/// is a file they did not ask for (ADR-033 decision 15), so the text goes to
/// the notes folder instead, as it does for a note that never had a file.
pub fn plan_orphan_recovery(file_is_there: bool) -> OrphanPlan {
    if file_is_there {
        OrphanPlan::BesideItsFile
    } else {
        OrphanPlan::InNotesFolder
    }
}

/// Outcome of comparing an entry of a 0.5 session snapshot against the
/// persisted buffer state.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryResolution {
    /// The snapshot is newer than the stored buffer; use snapshot content.
    Restore,
    /// The stored buffer is at least as recent; discard snapshot entry.
    Ignore,
    /// No snapshot entry exists for this buffer; nothing to do.
    NoSnapshot,
}

/// Compares timestamps to decide whether a snapshot entry should override
/// the stored buffer content.
///
/// The rule a 0.5 relaunch applied to its crash snapshot. It now decides only
/// which entries of a snapshot left by 0.5 move into the journal at the first
/// launch after the upgrade (ADR-044 §7).
///
/// `snapshot_created_at` and `buffer_updated_at` are ISO 8601 strings as
/// produced by SQLite's `datetime('now')`.
pub fn resolve_recovery(snapshot_created_at: &str, buffer_updated_at: &str) -> RecoveryResolution {
    if snapshot_created_at > buffer_updated_at {
        RecoveryResolution::Restore
    } else {
        RecoveryResolution::Ignore
    }
}

/// Text a note's save could not write, as the unsaved-text journal keeps it
/// (ADR-044).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsavedText {
    /// The entry's own id, which tells two entries of one note apart.
    pub entry_id: i64,
    /// The note's buffer id.
    pub buffer_id: String,
    /// The note's file when the entry was written, for the log. The launch
    /// writes to the file the note's row names now, which a move since then
    /// has changed.
    pub path: Option<String>,
    /// What the editor held.
    pub text: String,
    /// When the entry was written.
    pub recorded_at: DateTime<Utc>,
    /// Whether the entry is the text of the tab that typed it, which that
    /// tab's next landed save clears. An entry no tab holds is no save's to
    /// clear, and stays until a launch puts it on disk (ADR-044 §3).
    pub in_tab: bool,
}

/// What became of the text a save carried.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveLanding {
    /// The note's file holds the text. Also a note with no file and no text,
    /// which had nothing to write.
    Landed,
    /// The note's file was left alone and the text was written beside it, as
    /// the copy a refused save leaves.
    WrittenBeside,
    /// The text reached no file: the file is not downloaded, read-only,
    /// missing, deleted, on a volume that is gone or that stopped answering,
    /// or the note's first file could not be made.
    NotLanded,
    /// The note is read-only (a generated document, a binary file's view), so
    /// a refused write carried nothing the person typed.
    ReadOnlyNote,
}

/// What a save's outcome does to the journal entry of the tab it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JournalStep {
    /// Keep the text: nothing else holds it.
    Record,
    /// Drop the entry: the text it held has reached a file, or newer text has.
    Clear,
    /// Leave the entry as it is.
    Leave,
}

/// Decides what a save's outcome does to the journal.
///
/// The journal is the one copy of text outside a file ADR-028 §1 allows, and
/// only for text that has not reached one, so a tab's entry lives exactly from
/// the save that could not land to the next save of that tab that does. A copy
/// beside the note is a file holding the text, so it clears the entry as a
/// landed save does. An entry no tab holds is not touched by any save
/// (ADR-044 §3).
pub fn journal_step(landing: SaveLanding) -> JournalStep {
    match landing {
        SaveLanding::Landed | SaveLanding::WrittenBeside => JournalStep::Clear,
        SaveLanding::NotLanded => JournalStep::Record,
        SaveLanding::ReadOnlyNote => JournalStep::Leave,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_note_whose_file_was_deleted_is_not_written_back_at_the_next_launch() {
        assert_eq!(plan_recovery(true, false), RecoveryPlan::Withhold);
    }

    #[test]
    fn a_note_that_never_had_a_file_is_written_to_the_one_minted_for_it() {
        assert_eq!(plan_recovery(false, false), RecoveryPlan::Write);
    }

    #[test]
    fn a_note_whose_file_is_still_there_goes_through_the_ordinary_guard() {
        assert_eq!(plan_recovery(true, true), RecoveryPlan::Write);
    }

    #[test]
    fn text_whose_row_is_gone_goes_beside_its_file_while_the_file_is_there() {
        assert_eq!(plan_orphan_recovery(true), OrphanPlan::BesideItsFile);
    }

    #[test]
    fn text_whose_row_and_file_are_gone_goes_to_the_notes_folder() {
        assert_eq!(plan_orphan_recovery(false), OrphanPlan::InNotesFolder);
    }

    #[test]
    fn a_launch_record_without_an_unmade_file_still_reads() {
        let read: RecoveredBuffer =
            serde_json::from_str(r#"{"id":"n","content":"t","removed_on_disk":true}"#)
                .expect("decode");
        assert_eq!(read.unmade_file, None);
        let unmade = RecoveredBuffer {
            unmade_file: Some(UnmadeFile {
                name: "2026-10-13.txt".to_string(),
                folder: "Writ".to_string(),
            }),
            ..read
        };
        let json = serde_json::to_value(&unmade).expect("encode");
        assert_eq!(json["unmade_file"]["name"], "2026-10-13.txt");
        assert_eq!(json["unmade_file"]["folder"], "Writ");
    }

    #[test]
    fn a_save_that_landed_clears_the_journal_entry() {
        assert_eq!(journal_step(SaveLanding::Landed), JournalStep::Clear);
    }

    #[test]
    fn text_written_beside_the_note_has_reached_a_file_and_clears_the_entry() {
        assert_eq!(journal_step(SaveLanding::WrittenBeside), JournalStep::Clear);
    }

    #[test]
    fn text_that_reached_no_file_is_recorded() {
        assert_eq!(journal_step(SaveLanding::NotLanded), JournalStep::Record);
    }

    #[test]
    fn a_read_only_note_leaves_the_journal_alone() {
        assert_eq!(journal_step(SaveLanding::ReadOnlyNote), JournalStep::Leave);
    }

    #[test]
    fn a_snapshot_entry_newer_than_its_row_moves_into_the_journal() {
        assert_eq!(
            resolve_recovery("2024-01-01 12:00:01", "2024-01-01 12:00:00"),
            RecoveryResolution::Restore
        );
        assert_eq!(
            resolve_recovery("2024-01-01 12:00:00", "2024-01-01 12:00:00"),
            RecoveryResolution::Ignore
        );
    }
}
