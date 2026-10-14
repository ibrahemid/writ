//! How much of a conversation one request carries.
//!
//! Nothing in Writ knows a model's context window, and a provider's model list
//! does not say it. The budget is an estimate per kind of endpoint instead: a
//! runtime on this machine is held to a small local context, and a hosted model
//! to a large one. The turns are fitted to what the estimate leaves after the
//! system prompt, the attached notes and the reply, oldest dropped first, so a
//! long conversation stops growing the request rather than being refused for it
//! by a hosted model or cut from the front, system prompt and all, by a local
//! runtime (ADR-040 section 8, amended).

use super::*;
use crate::prompt::estimate_tokens;

/// The context a runtime on this machine is assumed to have, in tokens.
pub const LOCAL_CONTEXT_TOKENS: usize = 8_192;

/// The context a hosted model is assumed to have, in tokens.
pub const HOSTED_CONTEXT_TOKENS: usize = 128_000;

/// What is held back for the reply from a local runtime's context.
const LOCAL_REPLY_RESERVE: usize = 2_048;

/// The tokens a request may spend on the conversation's turns.
///
/// What the endpoint's context holds, less the reply's share, the system
/// prompt and every attached note. Zero when the notes alone fill it; the
/// newest turn is sent regardless ([`fit_turns`]).
pub fn turn_budget(is_local: bool, system: &str, context: &[AttachedNote]) -> usize {
    let (window, reserve) = if is_local {
        (LOCAL_CONTEXT_TOKENS, LOCAL_REPLY_RESERVE)
    } else {
        (HOSTED_CONTEXT_TOKENS, MAX_REPLY_TOKENS as usize)
    };
    let notes: usize = context.iter().map(|note| estimate_tokens(&note.text)).sum();
    window
        .saturating_sub(reserve)
        .saturating_sub(estimate_tokens(system))
        .saturating_sub(notes)
}

/// The newest turns that fit in `budget` tokens, oldest first.
///
/// The newest turn always goes, because it is the message being sent. Turns
/// before it are kept newest first until the next one would not fit, and a
/// window that would then open on the model's own turn drops that turn too: a
/// conversation sent to the Messages API has to open on the person's.
pub fn fit_turns(turns: &[ChatTurn], budget: usize) -> Vec<ChatTurn> {
    let Some(newest) = turns.len().checked_sub(1) else {
        return Vec::new();
    };
    let mut spent = estimate_tokens(&turns[newest].content);
    let mut first = newest;
    while first > 0 {
        let cost = estimate_tokens(&turns[first - 1].content);
        if spent + cost > budget {
            break;
        }
        spent += cost;
        first -= 1;
    }
    while first < newest && turns[first].role != Role::User {
        first += 1;
    }
    turns[first..].to_vec()
}
