//! Which line ending a file on disk uses, and how to keep it.
//!
//! CodeMirror splits a document on `\r\n?|\n` and hands back `\n` for every
//! break, so a note written on Windows arrives as LF and would be saved as LF
//! — every line of the file rewritten by the act of opening it. The ending is
//! read off the bytes once, carried with the note, and re-applied to the
//! editor's text on the way back out, so the file keeps the convention its
//! author gave it.

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

/// The line ending a note's file uses.
///
/// Only the two conventions still in use are represented. A classic Mac file
/// (bare `\r`) reads as one long line to the same splitter CodeMirror uses,
/// so there is no ending to preserve and it is [`LineEnding::Lf`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LineEnding {
    /// `\n`. The default: a new note, an empty file, and a file with no line
    /// break in it all take it.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl LineEnding {
    /// The value as it is written to the database and to JSON.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "lf",
            Self::CrLf => "crlf",
        }
    }

    /// Reads back what [`Self::as_str`] wrote. An unrecognised value is
    /// [`LineEnding::Lf`], so a row written by a newer build never fails an
    /// open — the worst it costs is one file normalised to LF.
    #[must_use]
    pub fn from_str_or_default(value: &str) -> Self {
        match value {
            "crlf" => Self::CrLf,
            _ => Self::Lf,
        }
    }

    /// The ending most of `text`'s lines use.
    ///
    /// A mixed file keeps its majority: counting is what makes "one stray
    /// `\r\n` in a 400-line LF file" stay an LF file, and the reverse. A tie,
    /// and a file with no line break at all, is [`LineEnding::Lf`], which is
    /// also what a file Writ creates gets.
    #[must_use]
    pub fn detect(text: &str) -> Self {
        Self::detect_bytes(text.as_bytes())
    }

    /// [`Self::detect`] over bytes that have not been decoded.
    ///
    /// Both endings are ASCII, so the count is the same one the text form
    /// makes and a large file does not have to be turned into a `String`
    /// first just to be counted.
    #[must_use]
    pub fn detect_bytes(bytes: &[u8]) -> Self {
        let mut crlf = 0usize;
        let mut lf = 0usize;
        let mut previous = 0u8;
        for &byte in bytes {
            if byte == b'\n' {
                if previous == b'\r' {
                    crlf += 1;
                } else {
                    lf += 1;
                }
            }
            previous = byte;
        }
        if crlf > lf {
            Self::CrLf
        } else {
            Self::Lf
        }
    }

    /// Folds every line ending in `text` to `\n`.
    ///
    /// What a hash of a file's text is taken over, so the same note read from
    /// a CRLF file and typed into the editor compare equal.
    #[must_use]
    pub fn normalise(text: &str) -> Cow<'_, str> {
        if text.contains("\r\n") {
            Cow::Owned(text.replace("\r\n", "\n"))
        } else {
            Cow::Borrowed(text)
        }
    }

    /// Turns LF text back into this ending.
    ///
    /// `text` must already be normalised — it is the editor's document, which
    /// CodeMirror hands over with `\n` breaks. A `\r\n` already in it would be
    /// doubled otherwise, so the CRLF arm normalises first.
    #[must_use]
    pub fn apply(self, text: &str) -> Cow<'_, str> {
        match self {
            Self::Lf => Self::normalise(text),
            Self::CrLf => Cow::Owned(Self::normalise(text).replace('\n', "\r\n")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LineEnding;

    #[test]
    fn detects_lf_in_a_unix_file() {
        assert_eq!(LineEnding::detect("one\ntwo\nthree\n"), LineEnding::Lf);
    }

    #[test]
    fn detects_crlf_in_a_windows_file() {
        assert_eq!(
            LineEnding::detect("one\r\ntwo\r\nthree\r\n"),
            LineEnding::CrLf
        );
    }

    #[test]
    fn a_file_with_no_line_break_is_lf() {
        assert_eq!(LineEnding::detect("just one line"), LineEnding::Lf);
        assert_eq!(LineEnding::detect(""), LineEnding::Lf);
    }

    #[test]
    fn a_mixed_file_takes_the_majority() {
        assert_eq!(LineEnding::detect("a\r\nb\r\nc\nd\r\n"), LineEnding::CrLf);
        assert_eq!(LineEnding::detect("a\nb\nc\r\nd\n"), LineEnding::Lf);
    }

    #[test]
    fn a_tie_is_lf() {
        assert_eq!(LineEnding::detect("a\r\nb\n"), LineEnding::Lf);
    }

    #[test]
    fn a_bare_carriage_return_is_not_a_line_break() {
        assert_eq!(LineEnding::detect("a\rb\rc"), LineEnding::Lf);
    }

    #[test]
    fn detects_over_bytes_the_same_way_as_over_text() {
        for text in ["a\r\nb\r\n", "a\nb\n", "a\r\nb\n", "", "one line"] {
            assert_eq!(
                LineEnding::detect_bytes(text.as_bytes()),
                LineEnding::detect(text),
                "{text:?}"
            );
        }
    }

    #[test]
    fn normalise_folds_crlf_and_leaves_lf_borrowed() {
        assert_eq!(LineEnding::normalise("a\r\nb\r\n"), "a\nb\n");
        assert!(matches!(
            LineEnding::normalise("a\nb\n"),
            std::borrow::Cow::Borrowed(_)
        ));
    }

    #[test]
    fn apply_round_trips_a_crlf_file() {
        let original = "one\r\ntwo\r\nthree\r\n";
        let ending = LineEnding::detect(original);
        let normalised = LineEnding::normalise(original);
        assert_eq!(ending.apply(&normalised), original);
    }

    #[test]
    fn apply_leaves_lf_text_alone() {
        assert_eq!(LineEnding::Lf.apply("a\nb\n"), "a\nb\n");
    }

    #[test]
    fn apply_does_not_double_a_carriage_return_already_there() {
        assert_eq!(LineEnding::CrLf.apply("a\r\nb"), "a\r\nb");
    }

    #[test]
    fn round_trips_through_its_stored_value() {
        for ending in [LineEnding::Lf, LineEnding::CrLf] {
            assert_eq!(LineEnding::from_str_or_default(ending.as_str()), ending);
        }
        assert_eq!(LineEnding::from_str_or_default("cr"), LineEnding::Lf);
    }

    #[test]
    fn serialises_as_the_stored_value() {
        assert_eq!(
            serde_json::to_string(&LineEnding::CrLf).expect("serialise"),
            "\"crlf\""
        );
        assert_eq!(
            serde_json::from_str::<LineEnding>("\"lf\"").expect("deserialise"),
            LineEnding::Lf
        );
    }
}
