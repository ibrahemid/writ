//! A proposal keeps the shape of the file it changes.
//!
//! A model writes a note back as lines, and the parser joins them with `\n`
//! and ends on one. A CRLF note, or a note that ends without a newline, would
//! be rewritten whole by an apply of that text, while the card's diff showed
//! one changed line. The proposal takes the line ending and the final-newline
//! state of the text it was attached with, and the card counts any line whose
//! ending still changes.

use writ_core::chat::{parse_proposals, AttachedNote, Proposal};
use writ_core::diff::LineKind;

fn note(text: &str) -> AttachedNote {
    AttachedNote {
        path: "Plan.md".to_string(),
        prompt_path: "Plan.md".to_string(),
        text: text.to_string(),
        before_hash: "hash-of-Plan.md".to_string(),
    }
}

fn proposal_for(original: &str, body: &str) -> Proposal {
    let reply = format!(
        "Here it is.\n\n```writ-proposal path=\"Plan.md\" summary=\"Move the day\"\n{body}```\n"
    );
    let parsed = parse_proposals(&reply, &[note(original)], false);
    assert_eq!(parsed.dropped, Vec::new());
    assert_eq!(parsed.proposals.len(), 1);
    parsed.proposals[0].clone()
}

fn changed_lines(proposal: &Proposal, kind: LineKind) -> Vec<String> {
    proposal
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter())
        .filter(|line| line.kind == kind)
        .map(|line| line.text.clone())
        .collect()
}

#[test]
fn a_crlf_note_keeps_crlf() {
    let original = "# Plan\r\n\r\nShip on Friday.\r\nCall Sam.\r\n";
    let proposal = proposal_for(original, "# Plan\n\nShip on Thursday.\nCall Sam.\n");
    assert_eq!(
        proposal.new_content,
        "# Plan\r\n\r\nShip on Thursday.\r\nCall Sam.\r\n"
    );
    assert_eq!(
        changed_lines(&proposal, LineKind::Removed),
        ["Ship on Friday."]
    );
    assert_eq!(
        changed_lines(&proposal, LineKind::Added),
        ["Ship on Thursday."]
    );
}

#[test]
fn a_note_without_a_final_newline_does_not_gain_one() {
    let original = "Ship on Friday.\nCall Sam.";
    let proposal = proposal_for(original, "Ship on Thursday.\nCall Sam.\n");
    assert_eq!(proposal.new_content, "Ship on Thursday.\nCall Sam.");
    assert_eq!(
        changed_lines(&proposal, LineKind::Removed),
        ["Ship on Friday."]
    );
    assert_eq!(
        changed_lines(&proposal, LineKind::Added),
        ["Ship on Thursday."]
    );
}

#[test]
fn a_crlf_note_without_a_final_newline_keeps_both() {
    let original = "Ship on Friday.\r\nCall Sam.";
    let proposal = proposal_for(original, "Ship on Thursday.\nCall Sam.\n");
    assert_eq!(proposal.new_content, "Ship on Thursday.\r\nCall Sam.");
}

#[test]
fn the_same_text_back_is_no_change_on_either_ending() {
    for original in ["One.\r\nTwo.\r\n", "One.\nTwo.", "One.\r\nTwo."] {
        let proposal = proposal_for(original, "One.\nTwo.\n");
        assert_eq!(proposal.new_content, original, "{original:?}");
        assert!(proposal.hunks.is_empty(), "{original:?}");
    }
}

#[test]
fn a_line_whose_ending_still_changes_is_counted() {
    // A mixed file keeps its majority ending, so the one LF line among CRLF
    // ones is rewritten. The text reads the same, and the card still shows the
    // line, because the file changes there.
    let original = "a\r\nb\r\nc\n";
    let proposal = proposal_for(original, "a\nb\nc\n");
    assert_eq!(proposal.new_content, "a\r\nb\r\nc\r\n");
    assert_eq!(changed_lines(&proposal, LineKind::Removed), ["c"]);
    assert_eq!(changed_lines(&proposal, LineKind::Added), ["c"]);
}

#[test]
fn an_empty_note_takes_the_text_as_the_model_wrote_it() {
    let proposal = proposal_for("", "A first line.\n");
    assert_eq!(proposal.new_content, "A first line.\n");
}
