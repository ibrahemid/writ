//! Replacing one passage of a note: the exact text a caller names, found once.
//!
//! A program that changes a sentence should not have to send the whole note
//! back, and must not change a passage it did not mean. So the text to replace
//! is matched exactly and has to be in the note once: absent, or there more
//! than once, nothing is replaced and the caller is told which.
//!
//! The one allowance is line endings. A caller that read a CRLF note through
//! something that speaks only `\n` sends `\n` lines; when the text is not in
//! the note as given and the note is CRLF, it is looked for again with `\r\n`
//! breaks. The new text takes the note's line ending, so the lines around the
//! passage are left as they were.

use std::borrow::Cow;

use super::line_ending::LineEnding;

/// A note's text with one passage replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    /// The whole text after the replacement.
    pub text: String,
    /// The 1-based line the new text starts on.
    pub line: u32,
}

/// Why nothing was replaced.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PassageError {
    /// The text to replace is empty, which is in every note everywhere.
    #[error("the text to replace is empty")]
    Empty,
    /// The note does not hold the text to replace.
    #[error("the text to replace is not in the file")]
    Absent,
    /// The note holds the text to replace more than once.
    #[error("the text to replace is in the file {count} times")]
    Repeated {
        /// How many places hold it, overlapping ones included.
        count: usize,
    },
}

/// `text` with `old`, found exactly once, replaced by `new`.
///
/// # Errors
///
/// [`PassageError`]: `old` is empty, absent or there more than once.
pub fn replace_passage(text: &str, old: &str, new: &str) -> Result<Replaced, PassageError> {
    if old.is_empty() {
        return Err(PassageError::Empty);
    }
    let ending = LineEnding::detect(text);
    let mut old = Cow::Borrowed(old);
    let mut found = places(text, &old);
    if found.is_empty() && ending == LineEnding::CrLf && old.contains('\n') {
        old = Cow::Owned(in_crlf(&old));
        found = places(text, &old);
    }
    let at = match found.as_slice() {
        [] => return Err(PassageError::Absent),
        [at] => *at,
        many => return Err(PassageError::Repeated { count: many.len() }),
    };
    let new = match ending {
        LineEnding::CrLf => Cow::Owned(in_crlf(new)),
        LineEnding::Lf => Cow::Borrowed(new),
    };
    let line = 1 + text[..at].matches('\n').count();
    Ok(Replaced {
        text: format!("{}{}{}", &text[..at], new, &text[at + old.len()..]),
        line: u32::try_from(line).unwrap_or(u32::MAX),
    })
}

/// Every byte offset `needle` starts at in `text`, overlapping ones included.
fn places(text: &str, needle: &str) -> Vec<usize> {
    let mut found = Vec::new();
    let mut from = 0;
    while let Some(offset) = text[from..].find(needle) {
        let at = from + offset;
        found.push(at);
        from = at + text[at..].chars().next().map_or(1, char::len_utf8);
    }
    found
}

/// `text` with every line break written `\r\n`.
fn in_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}
