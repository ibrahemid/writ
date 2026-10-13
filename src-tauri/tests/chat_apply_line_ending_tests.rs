//! A proposal applied to a note in the folder keeps the note's line ending and
//! its final-newline state.
//!
//! A model writes a note back as `\n` lines ending on a newline. Written as it
//! came, one fixed word in a CRLF note turned every line ending to LF, and a
//! note that ended without a newline gained one. The reply is read against the
//! bytes the request attached, and Apply writes the text the card showed.

use tempfile::TempDir;
use writ_core::chat::{parse_proposals, AttachedNote};
use writ_tauri_lib::commands::chat::apply_proposal_inner;
use writ_tauri_lib::watcher::open_files::NoOpenNotes;

/// Attaches `original` as a note, reads a reply proposing `proposed` for it,
/// applies that proposal, and answers the bytes the note then holds.
fn propose_and_apply(original: &[u8], proposed: &str) -> Vec<u8> {
    let dir = TempDir::new().expect("temp dir");
    let notes = std::fs::canonicalize(dir.path()).expect("canonical root");
    let writ = notes.join(".writ");
    std::fs::create_dir_all(&writ).expect("writ folder");

    let note = notes.join("Plan.md");
    std::fs::write(&note, original).expect("seed a note");

    let attached = AttachedNote {
        path: "Plan.md".to_string(),
        prompt_path: "Plan.md".to_string(),
        text: String::from_utf8(original.to_vec()).expect("text"),
        before_hash: writ_core::hash::sha256_hex(original),
    };
    let reply = format!("Here it is.\n\n```writ-proposal path=\"Plan.md\"\n{proposed}```\n");
    let parsed = parse_proposals(&reply, &[attached], false);
    assert_eq!(parsed.proposals.len(), 1, "dropped: {:?}", parsed.dropped);
    let proposal = &parsed.proposals[0];

    let outcome = apply_proposal_inner(
        &notes,
        &NoOpenNotes,
        &writ,
        "probe-host",
        &note.to_string_lossy(),
        &proposal.new_content,
        &proposal.before_hash,
        None,
        |_, _, _| unreachable!("a note in the folder is written through the host"),
    )
    .expect("the proposal applies");
    let written = std::fs::read(&note).expect("read back");
    assert_eq!(
        written,
        proposal.new_content.as_bytes(),
        "Apply wrote other bytes than the card showed"
    );
    assert_eq!(outcome.bytes, written.len() as u64);
    assert_eq!(outcome.hash, writ_core::hash::sha256_hex(&written));
    written
}

#[test]
fn a_crlf_note_stays_crlf() {
    let written = propose_and_apply(
        b"# Plan\r\n\r\nShip on Friday.\r\nCall Sam.\r\n",
        "# Plan\n\nShip on Thursday.\nCall Sam.\n",
    );
    assert_eq!(
        String::from_utf8(written).expect("text"),
        "# Plan\r\n\r\nShip on Thursday.\r\nCall Sam.\r\n"
    );
}

#[test]
fn a_note_without_a_final_newline_does_not_gain_one() {
    let written = propose_and_apply(
        b"Ship on Friday.\nCall Sam.",
        "Ship on Thursday.\nCall Sam.\n",
    );
    assert_eq!(
        String::from_utf8(written).expect("text"),
        "Ship on Thursday.\nCall Sam."
    );
}

#[test]
fn a_crlf_note_without_a_final_newline_keeps_both() {
    let written = propose_and_apply(
        b"Ship on Friday.\r\nCall Sam.",
        "Ship on Thursday.\nCall Sam.\n",
    );
    assert_eq!(
        String::from_utf8(written).expect("text"),
        "Ship on Thursday.\r\nCall Sam."
    );
}

#[test]
fn an_lf_note_with_a_final_newline_is_written_as_proposed() {
    let written = propose_and_apply(b"Ship on Friday.\n", "Ship on Thursday.\n");
    assert_eq!(
        String::from_utf8(written).expect("text"),
        "Ship on Thursday.\n"
    );
}
