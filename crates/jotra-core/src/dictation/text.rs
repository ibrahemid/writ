//! Tidying a transcript before it is inserted, and the context sent with the
//! next segment.

/// The instruction the clean-up pass sends to the AI connection.
pub const CLEAN_UP_INSTRUCTION: &str = "Fix punctuation, capitalization and misheard words in this dictated text. Keep the wording and the language. Return only the text.";

/// The transcript trimmed, or `None` when nothing in it is a letter or digit.
pub fn clean_transcript(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    trimmed
        .chars()
        .any(char::is_alphanumeric)
        .then(|| trimmed.to_string())
}

/// The last `max_chars` characters of `previous`, cut on a character
/// boundary.
pub fn prompt_tail(previous: &str, max_chars: usize) -> &str {
    if max_chars == 0 {
        return "";
    }
    match previous.char_indices().rev().nth(max_chars - 1) {
        Some((start, _)) => &previous[start..],
        None => previous,
    }
}
