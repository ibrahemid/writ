//! What a change outside Writ does to the tab holding the file.
//!
//! Two decisions live here. The first is whether the file's text may replace
//! the document at all: it may when the document holds nothing the file does
//! not, and it must be asked about when it holds something that would go
//! (ADR-033 §6 defines the difference). The second is what each answer to that
//! question writes, and it has one rule — the text that loses is written to
//! its own file before the winner is applied, so no answer ends with a text
//! that exists nowhere (spec W5, ADR-028 §5).
//!
//! Both are decisions and nothing else. Reading the file, writing the copy
//! beside the note and replacing the document are `writ-storage`'s and the
//! editor's halves.

use serde::{Deserialize, Serialize};

/// What happens to a document whose file changed under it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReloadPlan {
    /// Take the file's text. The document holds nothing that would be lost.
    ReplaceQuietly,
    /// Ask first. Replacing would discard text no file holds.
    Ask,
    /// Do nothing to the document.
    Ignore,
}

/// Decides what a change to a note's file does to the document in front of it.
///
/// `dirty` is the editor's answer to whether the document differs from the
/// file, which fails closed: a note Writ holds no record of reads dirty, and
/// so lands on [`ReloadPlan::Ask`] rather than having its text replaced.
///
/// `removed` outranks `changed`. A file that is gone has no text to take, and
/// the tab keeps its own and stops saving instead (ADR-033 §11); asking the
/// user to choose between their text and nothing is not a question.
///
/// A change that turned out to change nothing is ignored rather than replayed
/// through the editor. The watcher reports a write, not a difference, and a
/// sync client rewriting a file with its own bytes is the ordinary case.
pub fn plan_reload(dirty: bool, changed: bool, removed: bool) -> ReloadPlan {
    if removed || !changed {
        return ReloadPlan::Ignore;
    }
    if dirty {
        ReloadPlan::Ask
    } else {
        ReloadPlan::ReplaceQuietly
    }
}

/// The answer to [`ReloadPlan::Ask`], in the user's words.
///
/// The wire form is what the editor sends: `keep_mine`, `use_disk`,
/// `keep_both`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeChoice {
    /// The document wins the file. The file's text is kept beside it.
    KeepMine,
    /// The file wins the document. The document's text is kept beside it.
    UseDisk,
    /// The file wins the document and the document's text is opened too.
    KeepBoth,
}

/// One of the two texts a choice decides between.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    /// What the editor holds.
    Mine,
    /// What the file holds.
    Disk,
}

/// What is done to the note once the losing text has its own file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Write the document's text to the note's file.
    WriteMine,
    /// Put the file's text into the document.
    TakeDisk,
    /// Put the file's text into the document and open the copy as well.
    TakeDiskAndShowCopy,
}

impl Action {
    /// Which text the note's own file holds once the action has run.
    ///
    /// The other one is the copy's, which is what makes
    /// [`ChoiceOutcome::keeps_both_texts`] answerable.
    pub fn note_file_ends_with(self) -> Side {
        match self {
            Action::WriteMine => Side::Mine,
            Action::TakeDisk | Action::TakeDiskAndShowCopy => Side::Disk,
        }
    }
}

/// What a choice writes, and in which order.
///
/// The copy is written first. A write that lands and then fails to be copied
/// has already destroyed the text it was covering, and the order is the only
/// thing that rules that out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ChoiceOutcome {
    /// The text written beside the note before anything else happens.
    pub write_conflict_copy_of: Side,
    /// What happens to the note once that copy is on disk.
    pub then: Action,
}

impl ChoiceOutcome {
    /// Whether both texts are on disk once this outcome has run.
    ///
    /// True for every choice, which is the point: the copy holds whichever
    /// text the note's file does not.
    pub fn keeps_both_texts(self) -> bool {
        self.write_conflict_copy_of != self.then.note_file_ends_with()
    }
}

/// Decides what a choice writes.
///
/// Every arm writes a copy. The alternative — a choice that only replaces —
/// is how a person loses an afternoon's typing to a sync client that wrote
/// its own version of the file while they were out; the copy is the whole
/// reason the question is safe to ask (spec 205).
pub fn apply_choice(choice: ChangeChoice) -> ChoiceOutcome {
    match choice {
        ChangeChoice::KeepMine => ChoiceOutcome {
            write_conflict_copy_of: Side::Disk,
            then: Action::WriteMine,
        },
        ChangeChoice::UseDisk => ChoiceOutcome {
            write_conflict_copy_of: Side::Mine,
            then: Action::TakeDisk,
        },
        ChangeChoice::KeepBoth => ChoiceOutcome {
            write_conflict_copy_of: Side::Mine,
            then: Action::TakeDiskAndShowCopy,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every combination of the three inputs, in the order the table below
    /// reads them.
    const TABLE: [(bool, bool, bool, ReloadPlan); 8] = [
        (false, false, false, ReloadPlan::Ignore),
        (false, false, true, ReloadPlan::Ignore),
        (false, true, false, ReloadPlan::ReplaceQuietly),
        (false, true, true, ReloadPlan::Ignore),
        (true, false, false, ReloadPlan::Ignore),
        (true, false, true, ReloadPlan::Ignore),
        (true, true, false, ReloadPlan::Ask),
        (true, true, true, ReloadPlan::Ignore),
    ];

    #[test]
    fn the_whole_truth_table_answers_the_way_it_is_written() {
        for (dirty, changed, removed, expected) in TABLE {
            assert_eq!(
                plan_reload(dirty, changed, removed),
                expected,
                "dirty={dirty} changed={changed} removed={removed}"
            );
        }
    }

    #[test]
    fn the_table_covers_every_combination_of_the_three_inputs() {
        // A row added to the policy without a row here would be untested, and
        // the row that goes missing is the one nobody thought of.
        let mut seen: Vec<(bool, bool, bool)> =
            TABLE.iter().map(|(a, b, c, _)| (*a, *b, *c)).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 8);
    }

    #[test]
    fn an_unsaved_document_is_never_replaced_without_being_asked_about() {
        // The one row the whole feature exists for. Nothing may turn it into
        // ReplaceQuietly, whatever else changes.
        assert_eq!(plan_reload(true, true, false), ReloadPlan::Ask);
    }

    #[test]
    fn a_file_that_is_gone_is_never_asked_about() {
        for dirty in [true, false] {
            for changed in [true, false] {
                assert_eq!(plan_reload(dirty, changed, true), ReloadPlan::Ignore);
            }
        }
    }

    #[test]
    fn every_choice_leaves_both_texts_on_disk() {
        for choice in [
            ChangeChoice::KeepMine,
            ChangeChoice::UseDisk,
            ChangeChoice::KeepBoth,
        ] {
            let outcome = apply_choice(choice);
            assert!(outcome.keeps_both_texts(), "{choice:?} loses a text");
        }
    }

    #[test]
    fn keeping_mine_copies_the_file_and_writes_the_document() {
        assert_eq!(
            apply_choice(ChangeChoice::KeepMine),
            ChoiceOutcome {
                write_conflict_copy_of: Side::Disk,
                then: Action::WriteMine,
            }
        );
    }

    #[test]
    fn taking_the_file_copies_the_document_first() {
        assert_eq!(
            apply_choice(ChangeChoice::UseDisk),
            ChoiceOutcome {
                write_conflict_copy_of: Side::Mine,
                then: Action::TakeDisk,
            }
        );
    }

    #[test]
    fn showing_both_takes_the_file_and_opens_the_copy_of_the_document() {
        // The difference from UseDisk is the second tab, not the writing: the
        // document's text is written beside the note either way, so nothing
        // rests on the person having pressed the button that opens it.
        let outcome = apply_choice(ChangeChoice::KeepBoth);
        assert_eq!(outcome.write_conflict_copy_of, Side::Mine);
        assert_eq!(outcome.then, Action::TakeDiskAndShowCopy);
        assert_eq!(
            outcome.then.note_file_ends_with(),
            apply_choice(ChangeChoice::UseDisk)
                .then
                .note_file_ends_with()
        );
    }

    #[test]
    fn the_choices_travel_as_the_words_the_editor_sends() {
        assert_eq!(
            serde_json::to_value(ChangeChoice::KeepMine).unwrap(),
            "keep_mine"
        );
        assert_eq!(
            serde_json::to_value(ChangeChoice::UseDisk).unwrap(),
            "use_disk"
        );
        assert_eq!(
            serde_json::to_value(ChangeChoice::KeepBoth).unwrap(),
            "keep_both"
        );
        assert_eq!(
            serde_json::from_str::<ChangeChoice>("\"use_disk\"").unwrap(),
            ChangeChoice::UseDisk
        );
    }
}
