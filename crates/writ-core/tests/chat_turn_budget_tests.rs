//! How much of a conversation one request carries (ADR-040 section 8, amended).

use writ_core::chat::{
    fit_turns, turn_budget, AttachedNote, ChatTurn, Role, HOSTED_CONTEXT_TOKENS,
    LOCAL_CONTEXT_TOKENS,
};
use writ_core::prompt::estimate_tokens;

fn turn(role: Role, content: &str) -> ChatTurn {
    ChatTurn {
        role,
        content: content.to_string(),
    }
}

/// A conversation of `pairs` exchanges and the question being sent.
fn long_chat(pairs: usize, words: usize) -> Vec<ChatTurn> {
    let filler = "word ".repeat(words);
    let mut turns = Vec::new();
    for index in 0..pairs {
        turns.push(turn(Role::User, &format!("question {index} {filler}")));
        turns.push(turn(Role::Assistant, &format!("answer {index} {filler}")));
    }
    turns.push(turn(Role::User, "and now?"));
    turns
}

fn note(text: &str) -> AttachedNote {
    AttachedNote {
        path: "A.md".to_string(),
        prompt_path: "A.md".to_string(),
        text: text.to_string(),
        before_hash: "abc".to_string(),
    }
}

#[test]
fn a_conversation_inside_the_budget_is_sent_whole() {
    let turns = long_chat(3, 5);
    assert_eq!(fit_turns(&turns, 10_000), turns);
}

#[test]
fn the_oldest_turns_go_first_and_the_newest_always_stays() {
    let turns = long_chat(40, 200);
    let budget = 2_000;
    let fitted = fit_turns(&turns, budget);
    assert!(fitted.len() < turns.len());
    assert_eq!(
        fitted.last(),
        turns.last(),
        "the message being sent is kept"
    );
    assert_eq!(
        fitted[..],
        turns[turns.len() - fitted.len()..],
        "what is kept is the newest run, in order"
    );
    let spent: usize = fitted.iter().map(|one| estimate_tokens(&one.content)).sum();
    assert!(spent <= budget, "{spent} tokens over a budget of {budget}");
}

#[test]
fn a_fitted_conversation_opens_on_the_persons_turn() {
    let turns = long_chat(40, 200);
    for budget in [300, 700, 1_100, 1_900, 2_500] {
        let fitted = fit_turns(&turns, budget);
        assert_eq!(fitted[0].role, Role::User, "budget {budget}");
    }
}

#[test]
fn a_budget_of_nothing_still_sends_the_message() {
    let turns = long_chat(5, 50);
    assert_eq!(fit_turns(&turns, 0), vec![turn(Role::User, "and now?")]);
    assert!(fit_turns(&[], 100).is_empty());
}

#[test]
fn the_budget_is_what_the_window_leaves_after_the_prompt_and_the_notes() {
    let empty = turn_budget(false, "", &[]);
    assert!(empty < HOSTED_CONTEXT_TOKENS);
    let local = turn_budget(true, "", &[]);
    assert!(local < LOCAL_CONTEXT_TOKENS);
    assert!(local < empty);

    let text = "word ".repeat(2_000);
    let with_note = turn_budget(false, "a system prompt", &[note(&text)]);
    assert_eq!(
        with_note,
        empty - estimate_tokens("a system prompt") - estimate_tokens(&text)
    );
    let flooded = turn_budget(true, "", &[note(&"word ".repeat(20_000))]);
    assert_eq!(
        flooded, 0,
        "notes that fill a local window leave nothing for turns"
    );
}
