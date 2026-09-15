//! The stream filter that keeps a proposal out of the visible reply.

use writ_core::chat::{parse_proposals, AttachedNote, ProposalFilter};

/// A reply that holds prose, one proposal of three lines, and more prose.
const REPLY: &str = "Here is what I would change.\n\
\n\
```writ-proposal path=\"Ideas/Launch.md\" summary=\"Fold the intros\"\n\
The first proposed line.\n\
The second proposed line.\n\
The third proposed line.\n\
```\n\
\n\
That keeps the two intros together.\n";

/// The three lines the proposal holds, which must never be shown.
const PROPOSED: [&str; 3] = [
    "The first proposed line.",
    "The second proposed line.",
    "The third proposed line.",
];

/// Feeds a fresh filter the given pieces in order and returns all it released.
fn feed(pieces: &[&str]) -> String {
    let mut filter = ProposalFilter::new();
    let mut out = String::new();
    for piece in pieces {
        out.push_str(&filter.push(piece));
    }
    out.push_str(&filter.finish());
    out
}

/// What `text` filters to, asserted equal for every two-piece split on a char
/// boundary and for a run fed one character at a time.
fn every_split(text: &str) -> String {
    let whole = feed(&[text]);
    let boundaries = text
        .char_indices()
        .map(|(index, _)| index)
        .chain(std::iter::once(text.len()));
    for index in boundaries {
        assert_eq!(
            feed(&[&text[..index], &text[index..]]),
            whole,
            "split at byte {index}"
        );
    }
    let chars: Vec<String> = text.chars().map(|c| c.to_string()).collect();
    let pieces: Vec<&str> = chars.iter().map(String::as_str).collect();
    assert_eq!(feed(&pieces), whole, "one character at a time");
    whole
}

#[test]
fn a_proposal_is_withheld_at_every_byte_boundary() {
    let visible = every_split(REPLY);
    assert_eq!(
        visible,
        "Here is what I would change.\n\n\nThat keeps the two intros together.\n"
    );
    assert!(!visible.contains('`'), "a fence character reached the pane");
    for line in PROPOSED {
        assert!(!visible.contains(line), "{line:?} reached the pane");
    }
}

#[test]
fn nothing_parse_proposals_reads_reaches_the_pane() {
    let context = vec![AttachedNote {
        path: "Ideas/Launch.md".to_string(),
        text: "old text\n".to_string(),
        before_hash: "abc".to_string(),
    }];
    let proposals = parse_proposals(REPLY, &context);
    assert_eq!(proposals.len(), 1);
    let visible = feed(&[REPLY]);
    for line in proposals[0].new_content.lines() {
        assert!(!visible.contains(line), "{line:?} reached the pane");
    }
}

#[test]
fn an_ordinary_code_block_is_released_unchanged() {
    let reply =
        "Try this.\n\n```rust\nfn main() {\n    println!(\"hi\");\n}\n```\n\nThat is all.\n";
    assert_eq!(every_split(reply), reply);
}

#[test]
fn an_indented_proposal_is_withheld_and_an_indented_code_block_is_not() {
    let withheld = "before\n   ```writ-proposal path=\"A.md\"\n   new\n```\nafter\n";
    assert_eq!(every_split(withheld), "before\nafter\n");
    let released = "before\n   ```text\n   new\n   ```\nafter\n";
    assert_eq!(every_split(released), released);
}

#[test]
fn a_backtick_run_that_is_not_a_fence_is_released() {
    let reply = "Call `x` and read ``a`` then:\n\n```json\n{\"a\": 1}\n```\n\n````writ-proposal\nnot a fence\n````\n";
    assert_eq!(every_split(reply), reply);
}

#[test]
fn a_released_backtick_run_does_not_hide_the_fence_on_the_next_line() {
    let reply = "```json\n{}\n```\n```writ-proposal path=\"A.md\"\nnew\n```\ndone\n";
    let visible = every_split(reply);
    assert_eq!(visible, "```json\n{}\n```\ndone\n");
    assert!(!visible.contains("new"));
}

#[test]
fn an_unterminated_proposal_is_dropped() {
    let reply = "Here you go.\n```writ-proposal path=\"A.md\"\nhalf a not";
    assert_eq!(every_split(reply), "Here you go.\n");
}

#[test]
fn a_buffered_prefix_that_never_became_a_fence_is_released_on_finish() {
    let mut filter = ProposalFilter::new();
    assert_eq!(filter.push("text\n``"), "text\n");
    assert_eq!(filter.finish(), "``");
}

#[test]
fn a_reply_with_two_proposals_keeps_only_the_prose() {
    let reply = "One.\n```writ-proposal path=\"A.md\"\na\n```\nTwo.\n```writ-proposal path=\"B.md\"\nb\n```\nThree.\n";
    assert_eq!(every_split(reply), "One.\nTwo.\nThree.\n");
}

#[test]
fn a_fence_inside_a_proposal_body_does_not_reopen_it() {
    let reply =
        "One.\n```writ-proposal path=\"A.md\"\n```writ-proposal path=\"B.md\"\nbody\n```\nTwo.\n";
    assert_eq!(every_split(reply), "One.\nTwo.\n");
}

#[test]
fn a_filter_is_reusable_after_finish() {
    let mut filter = ProposalFilter::new();
    assert_eq!(filter.push("```writ-proposal path=\"A.md\"\na\n"), "");
    assert_eq!(filter.finish(), "");
    assert_eq!(
        filter.push("```writ-proposal path=\"A.md\"\nb\n```\nok\n"),
        "ok\n"
    );
    assert_eq!(filter.finish(), "");
}

#[test]
fn text_with_no_fence_at_all_is_released_verbatim() {
    let reply = "Two paragraphs.\n\nThe second one, with a $ and an emoji 🎈.\n";
    assert_eq!(every_split(reply), reply);
}

#[test]
fn a_default_filter_matches_a_new_one() {
    assert_eq!(
        ProposalFilter::default().push("hello"),
        ProposalFilter::new().push("hello")
    );
}
