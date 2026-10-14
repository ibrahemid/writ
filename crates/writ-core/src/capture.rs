//! Quick capture: what typed into the capture note becomes at the end of
//! Today's File (ADR-055).

use crate::notes::line_ending::LineEnding;

/// The entry `typed` adds, with its line breaks folded to `\n`, the blank
/// lines before it and the blank space after it taken off. The first line
/// keeps its indent. `None` when there is nothing in it, which adds nothing.
#[must_use]
pub fn capture_entry(typed: &str) -> Option<String> {
    let folded = LineEnding::normalise(typed);
    let first = folded.find(|c: char| !c.is_whitespace())?;
    let line_start = folded[..first].rfind('\n').map_or(0, |at| at + 1);
    Some(folded[line_start..].trim_end().to_string())
}

/// `existing` with `entry` added as its last lines.
///
/// The entry starts on a line of its own and ends on a line break, in the
/// line ending most of `existing`'s lines use, so the file keeps its shape
/// and the next entry starts on a fresh line too. Nothing already in the file
/// is touched.
#[must_use]
pub fn append_entry(existing: &str, entry: &str) -> String {
    let ending = LineEnding::detect(existing);
    let mut text = String::with_capacity(existing.len() + entry.len() + 4);
    text.push_str(existing);
    if !existing.is_empty() && !existing.ends_with(['\n', '\r']) {
        text.push_str(ending.as_break());
    }
    text.push_str(&ending.apply(entry));
    text.push_str(ending.as_break());
    text
}
