//! What `parse_proposals` makes of replies local models actually wrote.
//!
//! Each fixture in `fixtures/chat-replies/` is one reply from Ollama to the
//! prompt this crate builds, with the launch note below attached. They are
//! recorded verbatim, trailing newline and all, because the shapes that break
//! the parser are exactly the ones a hand-typed sample tidies away.

use writ_core::chat::{parse_proposals, AttachedNote, DropReason, ParsedProposals, ProposalFilter};

/// The note every fixture was answered with, as it was attached.
const NOTE: &str = "# Launch checklist\n\
\n\
- [ ] Write the release notes\n\
- [ ] Check the download page\n\
- [ ] Post the announcement\n\
\n\
The launch is on Thursday. Bring the checklist to the stand-up.\n";

const LLAMA_0: &str = include_str!("fixtures/chat-replies/ollama-llama3.2_3b-0.md");
const LLAMA_1: &str = include_str!("fixtures/chat-replies/ollama-llama3.2_3b-1.md");
const LLAMA_2: &str = include_str!("fixtures/chat-replies/ollama-llama3.2_3b-2.md");
const QWEN_0: &str = include_str!("fixtures/chat-replies/ollama-qwen2.5-coder_0.5b-0.md");
const QWEN_1: &str = include_str!("fixtures/chat-replies/ollama-qwen2.5-coder_0.5b-1.md");
const QWEN_2: &str = include_str!("fixtures/chat-replies/ollama-qwen2.5-coder_0.5b-2.md");
const BARE_0: &str = include_str!("fixtures/chat-replies/ollama-llama3.2_3b-bare-0.md");
const BARE_1: &str = include_str!("fixtures/chat-replies/ollama-llama3.2_3b-bare-1.md");

fn attached() -> Vec<AttachedNote> {
    vec![AttachedNote {
        path: "Launch.md".to_string(),
        text: NOTE.to_string(),
        before_hash: "hash-of-Launch.md".to_string(),
    }]
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
fn a_reply_that_never_closed_its_fence_still_offers_the_note_it_wrote() {
    let parsed = parse_proposals(LLAMA_0, &attached(), false);
    let proposal = only(&parsed);
    assert_eq!(
        proposal.path, "Launch.md",
        "the model copied the example path, which names the attached note"
    );
    assert!(
        proposal.new_content.contains("- [ ] Bring snacks"),
        "the line the reply added is missing: {:?}",
        proposal.new_content
    );
    assert!(proposal.new_content.ends_with("stand-up.\n"));
    assert_eq!(proposal.summary, "Add 'Bring snacks' to the checklist");
}

#[test]
fn a_reply_that_rewrote_one_paragraph_offers_that_paragraph() {
    let parsed = parse_proposals(LLAMA_1, &attached(), false);
    let proposal = only(&parsed);
    assert_eq!(
        proposal.new_content,
        "The launch is on Thursday. Bring the checklist to the stand-up.\n"
    );
    assert_eq!(proposal.summary, "Rewrite the last paragraph");
}

#[test]
fn a_reply_whose_note_holds_a_code_block_keeps_the_block_whole() {
    let parsed = parse_proposals(LLAMA_2, &attached(), false);
    let proposal = only(&parsed);
    assert!(
        proposal
            .new_content
            .contains("```bash\n# Commands\ngit tag v1.0.0\n```"),
        "the inner fence ended the block early: {:?}",
        proposal.new_content
    );
    assert_eq!(
        proposal.summary, "Add Commands section with git tag command",
        "the summary must survive the recovery"
    );
}

#[test]
fn a_reply_that_copied_the_example_body_offers_nothing() {
    let parsed = parse_proposals(QWEN_0, &attached(), false);
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].named, "Ideas/Launch.md");
    assert_eq!(parsed.dropped[0].reason, DropReason::Placeholder);
}

#[test]
fn a_reply_that_answered_in_prose_proposes_nothing_and_drops_nothing() {
    let parsed = parse_proposals(QWEN_1, &attached(), false);
    assert_eq!(parsed, ParsedProposals::default());
}

#[test]
fn a_reply_that_wrote_one_filled_block_and_two_empty_ones_offers_none_of_them() {
    let parsed = parse_proposals(QWEN_2, &attached(), false);
    assert!(parsed.proposals.is_empty());
    let reasons: Vec<DropReason> = parsed.dropped.iter().map(|drop| drop.reason).collect();
    assert_eq!(
        reasons,
        vec![
            DropReason::Placeholder,
            DropReason::EmptyBody,
            DropReason::EmptyBody
        ],
        "an empty body is judged before a repeat, so neither empty block reads as a duplicate"
    );
    assert!(parsed
        .dropped
        .iter()
        .all(|drop| drop.named == "Ideas/Launch.md"));
}

/// The note the two bare-fence fixtures were answered with.
///
/// The conversation file records the attachment's path, byte count and hash
/// but not its text, so the text here is a stand-in of the right shape: the
/// path and the hash are the recorded ones, and nothing these two assertions
/// read depends on the old text.
fn sourdough() -> Vec<AttachedNote> {
    vec![AttachedNote {
        path: "Sourdough.md".to_string(),
        text: "# Sourdough\n\nThe starter lives in the fridge.\n".to_string(),
        before_hash: "79a5ec2906817bcf26efd083a70e900d7ee4a14ebc4be81535123b548ded3303".to_string(),
    }]
}

/// Everything a filter releases for `reply`, fed whole.
fn shown(reply: &str) -> String {
    let mut filter = ProposalFilter::new();
    let mut out = filter.push(reply);
    out.push_str(&filter.finish());
    out
}

#[test]
fn a_reply_that_put_the_header_under_a_bare_fence_offers_the_note_it_wrote() {
    let parsed = parse_proposals(BARE_0, &sourdough(), false);
    let proposal = only(&parsed);
    assert_eq!(proposal.path, "Sourdough.md");
    assert_eq!(
        proposal.summary, "Explanation of sourdough fermentation",
        "the header line carries the same attributes as an info string"
    );
    assert!(
        proposal
            .new_content
            .starts_with("1. Sourdough fermentation"),
        "the header line is not part of the body: {:?}",
        proposal.new_content
    );
    assert!(proposal
        .new_content
        .ends_with("balance of yeast and bacteria.\n"));
    assert!(
        !proposal.new_content.contains("writ-proposal"),
        "the header reached the body: {:?}",
        proposal.new_content
    );

    assert_eq!(
        shown(BARE_0),
        "No changes offered, I will wait for your instruction to offer changes for this note.",
        "the pane shows the prose after the block and nothing of the block"
    );
}

#[test]
fn a_bare_fence_header_that_closed_at_once_offers_nothing_and_shows_the_guide() {
    let parsed = parse_proposals(BARE_1, &sourdough(), false);
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped.len(), 1);
    assert_eq!(parsed.dropped[0].named, "Sourdough.md");
    assert_eq!(
        parsed.dropped[0].reason,
        DropReason::EmptyBody,
        "the fence closed before any body line, so there is no note to offer"
    );

    let visible = shown(BARE_1);
    let prose: String = BARE_1.lines().skip(3).collect::<Vec<&str>>().join("\n");
    assert_eq!(
        visible, prose,
        "the guide the model wrote as prose is the whole of what the pane shows"
    );
    assert!(!visible.contains('`'), "a fence character reached the pane");
}
