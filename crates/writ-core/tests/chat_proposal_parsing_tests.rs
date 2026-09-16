//! What `parse_proposals` reads out of a reply, and what it drops.
//!
//! The samples are the shapes local and hosted models actually write: a fence
//! of four backticks, a tilde fence, an indented fence, a body that holds a
//! code block of its own, and a path spelled the way the model remembers it
//! rather than the way the attached list spells it.

use writ_core::chat::{
    parse_proposals, resolve_proposal_path, AttachedNote, DropReason, ParsedProposals,
};

fn note(path: &str, text: &str) -> AttachedNote {
    AttachedNote {
        path: path.to_string(),
        text: text.to_string(),
        before_hash: format!("hash-of-{path}"),
    }
}

fn launch() -> Vec<AttachedNote> {
    vec![note("Ideas/Launch.md", "The old text.\n")]
}

fn only(parsed: &ParsedProposals) -> &writ_core::chat::Proposal {
    assert_eq!(
        parsed.dropped,
        Vec::new(),
        "nothing should have been dropped"
    );
    assert_eq!(parsed.proposals.len(), 1, "one proposal was expected");
    &parsed.proposals[0]
}

#[test]
fn a_four_backtick_fence_is_a_proposal() {
    let reply = "Here is the rewrite.\n\n\
````writ-proposal path=\"Ideas/Launch.md\" summary=\"Fold the intros\"\n\
The whole new text.\n\
````\n\n\
That folds the two intros together.\n";

    let parsed = parse_proposals(reply, &launch());
    let proposal = only(&parsed);
    assert_eq!(proposal.path, "Ideas/Launch.md");
    assert_eq!(proposal.new_content, "The whole new text.\n");
    assert_eq!(proposal.summary, "Fold the intros");
}

#[test]
fn a_tilde_fence_is_a_proposal() {
    let reply = "~~~writ-proposal path=\"Ideas/Launch.md\"\nThe whole new text.\n~~~\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).new_content, "The whole new text.\n");
}

#[test]
fn an_indented_fence_is_a_proposal() {
    let reply = "   ```writ-proposal path=\"Ideas/Launch.md\"\n   Indented body line.\n   ```\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).new_content, "   Indented body line.\n");
}

#[test]
fn whitespace_before_the_info_word_still_opens_a_proposal() {
    let reply = "``` writ-proposal path=\"Ideas/Launch.md\"\nThe whole new text.\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).new_content, "The whole new text.\n");
}

#[test]
fn a_proposal_body_may_contain_a_code_fence() {
    let reply = "````writ-proposal path=\"Ideas/Launch.md\"\n\
# Launch\n\
\n\
```sh\n\
cargo run\n\
```\n\
\n\
The end.\n\
````\n\
Done.\n";

    let parsed = parse_proposals(reply, &launch());
    let proposal = only(&parsed);
    assert_eq!(
        proposal.new_content, "# Launch\n\n```sh\ncargo run\n```\n\nThe end.\n",
        "the inner fence must not close the block"
    );
}

#[test]
fn a_closing_fence_must_be_at_least_as_long_as_the_one_that_opened_it() {
    let reply = "```writ-proposal path=\"Ideas/Launch.md\"\nbody\n`````\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).new_content, "body\n");
}

#[test]
fn a_path_in_single_quotes_names_a_note() {
    let reply = "```writ-proposal path='Ideas/Launch.md' summary='Tighten it'\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    let proposal = only(&parsed);
    assert_eq!(proposal.path, "Ideas/Launch.md");
    assert_eq!(proposal.summary, "Tighten it");
}

#[test]
fn a_bare_path_attribute_names_a_note() {
    let reply = "```writ-proposal path=Ideas/Launch.md\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).path, "Ideas/Launch.md");
}

#[test]
fn a_bare_basename_names_the_one_note_that_ends_that_way() {
    let reply = "```writ-proposal path=\"Launch.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    let proposal = only(&parsed);
    assert_eq!(proposal.path, "Ideas/Launch.md");
    assert_eq!(proposal.before_hash, "hash-of-Ideas/Launch.md");
}

#[test]
fn an_absolute_path_names_the_note_it_ends_with() {
    let reply = "```writ-proposal path=\"/Users/someone/Writ/Ideas/Launch.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).path, "Ideas/Launch.md");
}

#[test]
fn a_windows_spelled_path_names_the_same_note() {
    let reply = "```writ-proposal path=\"Ideas\\Launch.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).path, "Ideas/Launch.md");
}

#[test]
fn a_basename_two_attached_notes_share_is_dropped_as_ambiguous() {
    let context = vec![
        note("Ideas/Launch.md", "one\n"),
        note("Archive/Launch.md", "two\n"),
    ];
    let reply = "```writ-proposal path=\"Launch.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &context);
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].named, "Launch.md");
    assert_eq!(parsed.dropped[0].reason, DropReason::AmbiguousNote);
}

#[test]
fn a_shared_basename_still_resolves_through_the_folder_it_names() {
    let context = vec![
        note("Ideas/Launch.md", "one\n"),
        note("Archive/Launch.md", "two\n"),
    ];
    let reply = "```writ-proposal path=\"/Users/someone/Writ/Archive/Launch.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &context);
    assert_eq!(only(&parsed).path, "Archive/Launch.md");
}

#[test]
fn a_note_that_was_never_attached_is_dropped_with_its_name() {
    let reply = "```writ-proposal path=\"Secrets/Keys.md\"\nnew\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].named, "Secrets/Keys.md");
    assert_eq!(parsed.dropped[0].reason, DropReason::UnknownNote);
}

#[test]
fn an_unterminated_block_is_dropped_as_unterminated() {
    let reply = "Here you go.\n```writ-proposal path=\"Ideas/Launch.md\"\nhalf a no";

    let parsed = parse_proposals(reply, &launch());
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].named, "Ideas/Launch.md");
    assert_eq!(parsed.dropped[0].reason, DropReason::UnterminatedBlock);
}

#[test]
fn a_drop_carries_no_note_text_and_no_reply_text() {
    let reply = "Secret prose nobody may log.\n\
```writ-proposal path=\"Nope.md\"\n\
The whole new text of a note.\n\
```\n";

    let parsed = parse_proposals(reply, &launch());
    let recorded = format!("{:?}", parsed.dropped);
    assert!(!recorded.contains("Secret prose"));
    assert!(!recorded.contains("The whole new text"));
}

#[test]
fn a_reply_with_leading_and_trailing_prose_still_parses() {
    let reply = "First I read it.\n\nThen:\n\n\
```writ-proposal path=\"Ideas/Launch.md\" summary=\"Tighten\"\n\
new\n\
```\n\n\
Tell me if that reads better.\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(only(&parsed).new_content, "new\n");
}

#[test]
fn a_reply_written_with_carriage_returns_still_parses() {
    let reply = "```writ-proposal path=\"Ideas/Launch.md\"\r\nnew\r\n```\r\n";

    let parsed = parse_proposals(reply, &launch());
    assert_eq!(
        only(&parsed).new_content,
        "new\n",
        "the proposed text is read a line at a time, so it arrives in the editor's own endings"
    );
}

#[test]
fn an_ollama_shaped_whole_note_reply_parses() {
    let context = vec![note(
        "Projects/Writ.md",
        "# Writ\n\nA notes editor.\n\n## Install\n\n```sh\nbrew install writ\n```\n",
    )];
    let reply = "Sure! I folded the install line into the intro. Here's the whole note:\n\n\
````writ-proposal path=Projects/Writ.md summary=Fold the install line into the intro\n\
# Writ\n\
\n\
A notes editor. Install it with:\n\
\n\
```sh\n\
brew install writ\n\
```\n\
````\n\n\
Let me know if you'd like it shorter.\n";

    let parsed = parse_proposals(reply, &context);
    let proposal = only(&parsed);
    assert_eq!(proposal.path, "Projects/Writ.md");
    assert!(proposal.new_content.contains("brew install writ"));
    assert!(proposal.new_content.ends_with("```\n"));
    assert!(!proposal.hunks.is_empty(), "a changed note has hunks");
}

#[test]
fn a_deepseek_shaped_whole_note_reply_parses() {
    let context = vec![note("Ideas/Launch.md", "# Launch\n\nMarch.\n")];
    let reply = "I've rewritten the note below.\n\n\
```writ-proposal path=\"Ideas/Launch.md\" summary=\"Move the date to April\"\n\
# Launch\n\
\n\
April.\n\
```\n\n\
The date is the only change.\n";

    let parsed = parse_proposals(reply, &context);
    assert_eq!(only(&parsed).new_content, "# Launch\n\nApril.\n");
}

#[test]
fn two_proposals_in_one_reply_are_both_read() {
    let context = vec![note("A.md", "a\n"), note("B.md", "b\n")];
    let reply = "One.\n```writ-proposal path=\"A.md\"\nnew a\n```\n\
Two.\n````writ-proposal path=\"B.md\"\nnew b\n````\n";

    let parsed = parse_proposals(reply, &context);
    assert_eq!(parsed.proposals.len(), 2);
    assert_eq!(parsed.dropped, Vec::new());
}

#[test]
fn an_ordinary_code_block_is_not_a_proposal() {
    let reply = "```rust\nfn main() {}\n```\n";

    let parsed = parse_proposals(reply, &launch());
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped, Vec::new());
}

#[test]
fn resolving_a_path_hands_back_the_attached_note() {
    let context = vec![
        note("Ideas/Launch.md", "one\n"),
        note("Archive/Old.md", "two\n"),
    ];

    assert_eq!(
        resolve_proposal_path("Ideas/Launch.md", &context).map(|note| note.path.as_str()),
        Some("Ideas/Launch.md")
    );
    assert_eq!(
        resolve_proposal_path("Old.md", &context).map(|note| note.path.as_str()),
        Some("Archive/Old.md")
    );
    assert_eq!(resolve_proposal_path("Missing.md", &context), None);
    assert_eq!(resolve_proposal_path("", &context), None);
}
