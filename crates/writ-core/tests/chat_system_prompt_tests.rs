//! What the system prompt says, and what a model copying it would write.
//!
//! Small local models copy the example block rather than following it, so the
//! example has to be right for the request it is sent with: the path of a note
//! that is really attached, and a body nobody could mistake for a note.

use writ_core::chat::{
    parse_proposals, system_prompt, AttachedNote, DropReason, SYSTEM_PROMPT_HEAD,
};

fn note(path: &str) -> AttachedNote {
    AttachedNote {
        path: path.to_string(),
        text: "The old text.\n".to_string(),
        before_hash: format!("hash-of-{path}"),
    }
}

#[test]
fn the_example_names_the_first_attached_note() {
    let context = vec![note("Launch.md"), note("Ideas/Other.md")];
    let prompt = system_prompt(&context);
    assert!(prompt.starts_with(SYSTEM_PROMPT_HEAD));
    assert!(
        prompt.contains("```writ-proposal path=\"Launch.md\" summary="),
        "the example must name a note that is attached: {prompt}"
    );
    assert!(
        !prompt.contains("Ideas/Launch.md"),
        "the old example path is what models copied: {prompt}"
    );
}

#[test]
fn a_reply_that_copies_the_example_path_reaches_the_attached_note() {
    let context = vec![note("Launch.md")];
    let prompt = system_prompt(&context);
    let copied = prompt
        .lines()
        .find(|line| line.starts_with("```writ-proposal"))
        .expect("the example block");
    let reply = format!("{copied}\nThe whole note, rewritten.\n```\n");

    let parsed = parse_proposals(&reply, &context, false);
    assert_eq!(parsed.dropped, Vec::new());
    assert_eq!(parsed.proposals.len(), 1);
    assert_eq!(parsed.proposals[0].path, "Launch.md");
}

#[test]
fn a_reply_that_copies_the_example_body_is_dropped() {
    let context = vec![note("Launch.md")];
    let prompt = system_prompt(&context);
    let body = prompt
        .lines()
        .find(|line| line.starts_with('<'))
        .expect("the example body");
    let reply = format!("```writ-proposal path=\"Launch.md\"\n{body}\n```\n");

    let parsed = parse_proposals(&reply, &context, false);
    assert!(parsed.proposals.is_empty());
    assert_eq!(parsed.dropped[0].reason, DropReason::Placeholder);
}

#[test]
fn nothing_attached_says_so_and_names_no_real_note() {
    let prompt = system_prompt(&[]);
    assert!(prompt.starts_with(SYSTEM_PROMPT_HEAD));
    assert!(
        prompt.contains("No note is attached"),
        "a model with nothing to work on is told so: {prompt}"
    );
    assert!(prompt.contains("path=\"Notes/Example.md\""));
}

#[test]
fn a_path_holding_a_quote_keeps_its_own_quoting() {
    let context = vec![note("Ideas/\"Launch\".md")];
    let prompt = system_prompt(&context);
    assert!(
        prompt.contains("path='Ideas/\"Launch\".md'"),
        "a path with a double quote is quoted the other way: {prompt}"
    );
}
