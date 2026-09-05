//! The sentence a link sits in.
//!
//! The backlink list shows each link with the sentence around it (spec L2).
//! The sentence is cut out of the text the index already holds, never read
//! back off disk: a note the filesystem holds as a placeholder has no text in
//! the index, and reading one to fill a list would materialise the file the
//! walk was careful not to touch. Such a note yields an empty snippet and
//! keeps its row.

/// Longest snippet in characters. A markdown paragraph is one line, and a line
/// with no sentence punctuation in it is the whole paragraph, so a snippet
/// that is not bounded here is bounded by nothing.
pub const SNIPPET_MAX_CHARS: usize = 320;

/// Ends a sentence when whitespace or the end of the line follows it. `؟` is
/// here because an Arabic note ends a question with it and nothing else.
const TERMINATORS: [char; 4] = ['.', '!', '?', '؟'];

/// The sentence around the byte `offset` of `text`, trimmed, at most
/// [`SNIPPET_MAX_CHARS`] characters wide and windowed around `offset` when the
/// sentence is wider than that.
///
/// The sentence never crosses a line break: in markdown a line break is a
/// block boundary as often as it is a wrap, and a snippet that ran across one
/// would quote a heading into the paragraph under it. A line with no
/// terminator in it is one sentence.
///
/// A terminator is judged by what follows it, so `Note.md` and `1.5` stay
/// whole; `e.g.` does not, and the snippet then starts mid-abbreviation rather
/// than losing the link.
pub fn sentence_at(text: &str, offset: usize) -> String {
    if text.is_empty() {
        return String::new();
    }
    let offset = floor_char_boundary(text, offset.min(text.len()));
    let line_start = text[..offset].rfind('\n').map_or(0, |index| index + 1);
    let line_end = text[offset..]
        .find('\n')
        .map_or(text.len(), |index| offset + index);
    let line = text[line_start..line_end].trim_end_matches('\r');
    let relative = (offset - line_start).min(line.len());

    let (start, end) = sentence_bounds(line, relative);
    let raw = &line[start..end];
    let lead = raw.len() - raw.trim_start().len();
    let sentence = raw.trim();
    if sentence.is_empty() {
        let lead = line.len() - line.trim_start().len();
        return window(line.trim(), relative.saturating_sub(lead));
    }
    window(sentence, relative.saturating_sub(start + lead))
}

/// The byte range of the sentence of `line` that holds `relative`.
fn sentence_bounds(line: &str, relative: usize) -> (usize, usize) {
    let mut start = 0usize;
    let mut chars = line.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if !TERMINATORS.contains(&ch) {
            continue;
        }
        // `?!` and `...` end one sentence, not three.
        let mut end = index + ch.len_utf8();
        while let Some(&(next, following)) = chars.peek() {
            if !TERMINATORS.contains(&following) {
                break;
            }
            end = next + following.len_utf8();
            chars.next();
        }
        let after = &line[end..];
        if !after.is_empty() && !after.starts_with(char::is_whitespace) {
            continue;
        }
        if relative < end {
            return (start, end);
        }
        start = end;
    }
    (start, line.len())
}

/// `sentence` itself, or the [`SNIPPET_MAX_CHARS`] characters around the byte
/// `relative` when it is longer than that, so the link stays inside the
/// snippet that is supposed to show it.
fn window(sentence: &str, relative: usize) -> String {
    let total = sentence.chars().count();
    if total <= SNIPPET_MAX_CHARS {
        return sentence.to_string();
    }
    let boundary = floor_char_boundary(sentence, relative.min(sentence.len()));
    let at = sentence[..boundary].chars().count();
    let start = at
        .saturating_sub(SNIPPET_MAX_CHARS / 2)
        .min(total - SNIPPET_MAX_CHARS);
    sentence
        .chars()
        .skip(start)
        .take(SNIPPET_MAX_CHARS)
        .collect()
}

/// The nearest char boundary at or before `index`. `str::floor_char_boundary`
/// is unstable, and slicing a multi-byte character in half panics.
fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn takes_the_sentence_the_offset_is_in() {
        let text = "First one. Second holds [[Note]] here. Third one.";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "Second holds [[Note]] here.");
    }

    #[test]
    fn takes_the_first_sentence_of_a_line() {
        let text = "Holds [[Note]] here. Second one.";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "Holds [[Note]] here.");
    }

    #[test]
    fn takes_the_last_sentence_of_a_line_without_a_terminator() {
        let text = "First one. Trailing [[Note]]";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "Trailing [[Note]]");
    }

    #[test]
    fn never_crosses_a_line_break() {
        let text = "# Heading\nBody with [[Note]]\nNext line.\n";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "Body with [[Note]]");
    }

    #[test]
    fn keeps_a_line_with_no_terminator_whole() {
        let text = "- a list item mentioning [[Note]] and nothing else";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), text);
    }

    #[test]
    fn does_not_split_a_dot_inside_a_word() {
        let text = "See [[Note.md]] and version 1.5 of it.";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), text);
    }

    #[test]
    fn treats_a_run_of_terminators_as_one_ending() {
        let text = "Really?! Then [[Note]] said so.";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "Then [[Note]] said so.");
    }

    #[test]
    fn ends_an_arabic_question() {
        let text = "أين الملف؟ يوجد في [[Note]] هنا.";
        let offset = text.find("[[").expect("link");
        assert_eq!(sentence_at(text, offset), "يوجد في [[Note]] هنا.");
    }

    #[test]
    fn windows_a_sentence_longer_than_the_cap_around_the_link() {
        let padding = "x".repeat(SNIPPET_MAX_CHARS);
        let text = format!("{padding} [[Note]] {padding}");
        let offset = text.find("[[").expect("link");
        let snippet = sentence_at(&text, offset);
        assert_eq!(snippet.chars().count(), SNIPPET_MAX_CHARS);
        assert!(snippet.contains("[[Note]]"));
    }

    #[test]
    fn keeps_the_end_of_a_long_sentence_in_view() {
        let padding = "x".repeat(SNIPPET_MAX_CHARS * 2);
        let text = format!("{padding} [[Note]]");
        let offset = text.find("[[").expect("link");
        let snippet = sentence_at(&text, offset);
        assert_eq!(snippet.chars().count(), SNIPPET_MAX_CHARS);
        assert!(snippet.ends_with("[[Note]]"));
    }

    #[test]
    fn a_note_with_no_indexed_text_has_no_snippet() {
        assert_eq!(sentence_at("", 0), "");
    }

    #[test]
    fn an_offset_past_the_end_reads_the_last_line() {
        let text = "First line.\nLast line.";
        assert_eq!(sentence_at(text, text.len() + 99), "Last line.");
    }

    #[test]
    fn an_offset_inside_a_multibyte_character_does_not_panic() {
        let text = "قال [[Note]] هنا.";
        for offset in 0..=text.len() + 2 {
            let _ = sentence_at(text, offset);
        }
    }
}
