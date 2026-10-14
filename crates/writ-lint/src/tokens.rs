//! Splits a prose range into word tokens.

use std::ops::Range;

/// What a token is, for the rules that read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TokenKind {
    /// Letters, with apostrophes inside: `word`, `don't`, `écrit`, `مرحبا`.
    Word,
    /// Letters mixed with digits or underscores: `mp3`, `ext4`, `snake_case`.
    Alnum,
    /// A URL, an email address, a host name, a path or an HTML entity.
    Other,
}

/// One token, as byte offsets into the whole document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Token {
    pub start: usize,
    pub end: usize,
    pub kind: TokenKind,
}

impl Token {
    pub fn text<'a>(&self, text: &'a str) -> &'a str {
        &text[self.start..self.end]
    }
}

fn is_word_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_apostrophe(c: char) -> bool {
    c == '\'' || c == '\u{2019}'
}

/// True when every letter of `word` is an ASCII letter, apart from apostrophes:
/// the words the English dictionaries can judge.
pub(crate) fn is_latin_word(word: &str) -> bool {
    !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphabetic() || is_apostrophe(c))
}

/// True when the source between two offsets is whitespace only.
pub(crate) fn gap_is_whitespace(text: &str, from: usize, to: usize) -> bool {
    from <= to && text[from..to].chars().all(char::is_whitespace)
}

/// True when `ch` sits between two letters or digits somewhere in `segment`.
fn has_inner(segment: &[char], ch: char) -> bool {
    segment
        .windows(3)
        .any(|w| w[1] == ch && w[0].is_alphanumeric() && w[2].is_alphanumeric())
}

/// URLs, email addresses, host names, file names and paths: whole
/// whitespace-separated segments that are not prose.
fn is_link_like(segment: &[char]) -> bool {
    let joined: String = segment.iter().collect();
    joined.contains("://")
        || joined.to_ascii_lowercase().starts_with("www.")
        || has_inner(segment, '@')
        || has_inner(segment, '.')
        || has_inner(segment, '/')
        || has_inner(segment, '\\')
}

/// Splits the prose range `run` of `text` into tokens, in order.
///
/// A whitespace-separated segment shaped like a URL, an email address, a host
/// name or a path is one [`TokenKind::Other`] token. Elsewhere a token is a run
/// of letters, digits and underscores; an apostrophe between two of them stays
/// inside it, and a hyphen splits it.
pub(crate) fn tokenize(text: &str, run: &Range<usize>) -> Vec<Token> {
    let slice = &text[run.start..run.end];
    let chars: Vec<(usize, char)> = slice.char_indices().collect();
    let byte_at =
        |i: usize| -> usize { run.start + chars.get(i).map_or(slice.len(), |&(offset, _)| offset) };

    let mut tokens = Vec::new();
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i].1.is_whitespace() {
            i += 1;
            continue;
        }
        let seg_start = i;
        let mut seg_end = i;
        while seg_end < chars.len() && !chars[seg_end].1.is_whitespace() {
            seg_end += 1;
        }
        let segment: Vec<char> = chars[seg_start..seg_end].iter().map(|&(_, c)| c).collect();

        if is_link_like(&segment) {
            tokens.push(Token {
                start: byte_at(seg_start),
                end: byte_at(seg_end),
                kind: TokenKind::Other,
            });
            i = seg_end;
            continue;
        }

        let mut j = 0usize;
        while j < segment.len() {
            if !is_word_char(segment[j]) {
                j += 1;
                continue;
            }
            let word_start = j;
            let mut k = j + 1;
            loop {
                if k < segment.len() && is_word_char(segment[k]) {
                    k += 1;
                } else if k + 1 < segment.len()
                    && is_apostrophe(segment[k])
                    && segment[k + 1].is_alphanumeric()
                {
                    k += 2;
                } else {
                    break;
                }
            }
            let word = &segment[word_start..k];
            let is_entity = word_start > 0
                && (segment[word_start - 1] == '&'
                    || (segment[word_start - 1] == '#'
                        && word_start > 1
                        && segment[word_start - 2] == '&'))
                && segment.get(k) == Some(&';');
            let kind = if is_entity {
                TokenKind::Other
            } else if word.iter().any(|c| c.is_numeric() || *c == '_') {
                TokenKind::Alnum
            } else {
                TokenKind::Word
            };
            tokens.push(Token {
                start: byte_at(seg_start + word_start),
                end: byte_at(seg_start + k),
                kind,
            });
            j = k;
        }
        i = seg_end;
    }
    tokens
}
