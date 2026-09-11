//! The two permission sets the chat pane holds (ADR-032 section 5).
//!
//! The pane answers to two actors, so it holds two sets. The context builder is
//! the side a model's reply can influence and it carries no write, which is what
//! makes ADR-031 rule 4.3 a property of the type rather than a habit of the
//! code. Applying a proposal runs from the user's `Apply` and carries a write
//! and nothing else.

use tempfile::TempDir;
use writ_core::notes::host::{Capability, HostError, NoteHost};
use writ_core::notes::WriteOrigin;
use writ_storage::note_host::NoteHostImpl;
use writ_tauri_lib::commands::chat::{apply_permissions, context_permissions};

#[test]
fn the_context_set_cannot_write() {
    assert!(context_permissions().is_read_only());
}

#[test]
fn the_apply_set_is_the_write_and_nothing_else() {
    let applying = apply_permissions();

    let held: Vec<Capability> = Capability::ALL
        .iter()
        .copied()
        .filter(|capability| applying.contains(*capability))
        .collect();
    assert_eq!(held, vec![Capability::WriteNote]);
}

#[test]
fn a_write_through_the_context_set_is_refused_and_the_note_is_untouched() {
    let dir = TempDir::new().expect("temp dir");
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("notes folder");
    let note = notes.join("Launch.md");
    std::fs::write(&note, "before\n").expect("seed a note");

    let building = NoteHostImpl::open(&notes, None, context_permissions()).expect("open the host");
    let refused = building
        .write_note(
            "Launch.md",
            "what the model proposed\n",
            None,
            WriteOrigin::Chat,
        )
        .expect_err("the side a reply can influence has no write path");

    assert_eq!(
        refused,
        HostError::NotPermitted {
            capability: Capability::WriteNote
        }
    );
    assert_eq!(
        std::fs::read_to_string(&note).expect("read back"),
        "before\n"
    );
}
