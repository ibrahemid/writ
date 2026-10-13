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
/// Only the two conventions still in use are represented. CodeMirror's
/// splitter is `\r\n?|\n`, so a classic Mac file (bare `\r`) does open as many
/// lines — but [`Self::detect_bytes`] counts only `\r\n` against `\n`, so such
/// a file records [`LineEnding::Lf`] and its first save writes it out in LF.
/// That is the chosen answer: CR-only files are effectively extinct, and
/// carrying a third convention to keep one alive is not worth the surface.
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
    /// a CRLF file and typed into the editor compare equal. The breaks it
    /// folds are CodeMirror's own (`\r\n?|\n`), so a lone `\r` is a break here
    /// too and the result is what the editor would have shown.
    ///
    /// One pass, because two would be wrong: folding `\r\n` and then every
    /// remaining `\r` turns `a\r\r\n` into `a\n\n`, but folding `\r` first mints
    /// a fresh `\r\n` out of the pair and loses a line. The output holds no
    /// `\r` at all, so the function is idempotent and so is [`Self::apply`].
    #[must_use]
    pub fn normalise(text: &str) -> Cow<'_, str> {
        if !text.contains('\r') {
            return Cow::Borrowed(text);
        }
        let mut folded = String::with_capacity(text.len());
        let mut rest = text.chars().peekable();
        while let Some(character) = rest.next() {
            if character == '\r' {
                folded.push('\n');
                if rest.peek() == Some(&'\n') {
                    rest.next();
                }
            } else {
                folded.push(character);
            }
        }
        Cow::Owned(folded)
    }

    /// Turns LF text back into this ending.
    ///
    /// `text` need not be normalised: both arms fold first, so a carriage
    /// return already in the text is never doubled and applying twice gives
    /// the same bytes as applying once.
    #[must_use]
    pub fn apply(self, text: &str) -> Cow<'_, str> {
        match self {
            Self::Lf => Self::normalise(text),
            Self::CrLf => Cow::Owned(Self::normalise(text).replace('\n', "\r\n")),
        }
    }

    /// The break this ending writes.
    #[must_use]
    pub fn as_break(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

/// `text` in the shape of `original`: the line ending most of `original`'s
/// lines use, and whether `original` ends on a line break.
///
/// Text written back from somewhere that only speaks `\n` lines, such as a
/// model proposing a whole note, would otherwise rewrite every line ending of
/// a CRLF file and give a newline to a file that ended without one, so the
/// write changes lines nobody changed. Only one trailing break is added or
/// taken: a blank last line the text holds on purpose stays. An empty
/// `original` carries no evidence of either, so `text` keeps its own final
/// break and takes LF.
#[must_use]
pub fn in_shape_of(original: &str, text: &str) -> String {
    let ending = LineEnding::detect(original);
    let mut shaped = ending.apply(text).into_owned();
    if original.is_empty() {
        return shaped;
    }
    let ends_on_a_break = |text: &str| text.ends_with('\n');
    match (ends_on_a_break(original), ends_on_a_break(&shaped)) {
        (true, false) if !shaped.is_empty() => shaped.push_str(ending.as_break()),
        (false, true) => {
            let cut = if shaped.ends_with("\r\n") { 2 } else { 1 };
            shaped.truncate(shaped.len() - cut);
        }
        _ => {}
    }
    shaped
}

#[cfg(test)]
mod tests {
    use super::{in_shape_of, LineEnding};

    #[test]
    fn text_takes_a_crlf_file_s_ending_and_final_break() {
        assert_eq!(in_shape_of("a\r\nb\r\n", "a\nc\n"), "a\r\nc\r\n");
        assert_eq!(in_shape_of("a\r\nb", "a\nc\n"), "a\r\nc");
        assert_eq!(in_shape_of("a\r\nb\r\n", "a\nc"), "a\r\nc\r\n");
    }

    #[test]
    fn text_takes_an_lf_file_s_final_break_or_its_absence() {
        assert_eq!(in_shape_of("a\nb", "a\nc\n"), "a\nc");
        assert_eq!(in_shape_of("a\nb\n", "a\nc"), "a\nc\n");
        assert_eq!(in_shape_of("a\nb\n", "a\r\nc\r\n"), "a\nc\n");
    }

    #[test]
    fn only_one_final_break_is_taken() {
        assert_eq!(in_shape_of("a\nb", "a\nb\n\n"), "a\nb\n");
    }

    #[test]
    fn an_empty_file_keeps_the_text_as_it_came() {
        assert_eq!(in_shape_of("", "a\nb\n"), "a\nb\n");
        assert_eq!(in_shape_of("", "a"), "a");
    }

    #[test]
    fn empty_text_stays_empty() {
        assert_eq!(in_shape_of("a\r\n", ""), "");
    }

    #[test]
    fn shaping_twice_is_shaping_once() {
        for (original, text) in [("a\r\nb", "x\ny\n"), ("a\nb\n", "x\ny"), ("a\r\n", "x")] {
            let once = in_shape_of(original, text);
            assert_eq!(in_shape_of(original, &once), once, "{original:?} {text:?}");
        }
    }

    /// What lets a stored proposal be given its note's ending again when a
    /// conversation is opened: a shaped text already holds that ending, even
    /// where shaping it a second time would take another final break.
    #[test]
    fn a_shaped_text_already_holds_the_original_s_ending() {
        for (original, text) in [
            ("a\r\nb\r\n", "x\ny\n"),
            ("a\r\nb", "x\ny\n"),
            ("a\nb", "x\ny\n\n"),
            ("a\nb\n", "x\r\ny"),
            ("", "x\ny\n"),
            ("a\r\nb\nc", "x\ny\n"),
            ("a\r\nb\r\nc\n", "x\r\ny\nz\n"),
            ("one line", "x\ny\n"),
            ("a\rb\n", "x\ry\n"),
        ] {
            let shaped = in_shape_of(original, text);
            assert_eq!(
                LineEnding::detect(original).apply(&shaped),
                shaped,
                "{original:?} {text:?}"
            );
        }
    }

    #[test]
    fn each_ending_writes_its_own_break() {
        assert_eq!(LineEnding::Lf.as_break(), "\n");
        assert_eq!(LineEnding::CrLf.as_break(), "\r\n");
    }

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
    fn normalise_folds_a_lone_carriage_return() {
        assert_eq!(LineEnding::normalise("a\rb"), "a\nb");
        assert_eq!(LineEnding::normalise("\r"), "\n");
    }

    #[test]
    fn normalise_keeps_both_breaks_in_a_carriage_return_before_a_pair() {
        // Two breaks, not one: folding `\r\n` first and then the leftover
        // `\r` gives the same two, but folding `\r` first would mint a fresh
        // `\r\n` from the middle of the run and lose a line.
        assert_eq!(LineEnding::normalise("a\r\r\nb"), "a\n\nb");
        assert_eq!(LineEnding::normalise("\n\r"), "\n\n");
    }

    #[test]
    fn normalise_and_apply_are_idempotent() {
        for text in ["a\r\r\nb", "\n\r", "\r", "a\r\nb", "a\nb"] {
            let once = LineEnding::normalise(text).into_owned();
            assert_eq!(LineEnding::normalise(&once), once, "normalise {text:?}");
            for ending in [LineEnding::Lf, LineEnding::CrLf] {
                let once = ending.apply(text).into_owned();
                assert_eq!(ending.apply(&once), once, "{ending:?} apply {text:?}");
            }
        }
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
