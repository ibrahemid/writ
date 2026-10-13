//! Frame navigations a previewed document starts without a script, disarmed
//! before it is served.
//!
//! An `<iframe src>` or `<frame src>`, an `<iframe srcdoc>` and a
//! `<meta http-equiv=refresh>` each start a navigation as the document is
//! parsed. The app's `frame-src` and the document's `frame-src 'none'` refuse
//! one aimed outside the preview, but Chromium's browser layer, which WebView2
//! ships, opens a connection to the target before refusing it, and neither
//! refuses a `srcdoc` document: WebKit and Chromium both build it, from markup
//! no rewrite saw, so a resource hint inside it connects (ADR-043). The
//! document's own `frame-src 'none'` refuses every frame it could
//! legitimately show, so nothing it displays depends on these attributes.
//!
//! The rewrite gives each such attribute an inert name: `src` and `srcdoc` on
//! every `iframe` and `frame` tag, and `http-equiv` on every `meta` tag unless
//! its value is, as a whole and in plain text, one of [`INERT_PRAGMAS`].
//! `refresh` is the pragma that navigates; keeping only pragmas known to set
//! no navigation and open no connection, as the hint rewrite keeps only
//! stylesheet links, covers any other an engine acts on. [`DISARMED_PREFIX`]
//! goes in front of the name, so the element, its other attributes and the
//! rest of the document parse as before, and no reader finds the attribute,
//! whichever of two same-named ones it takes: every copy is renamed.
//!
//! A name right after `</` is also an end tag's name wherever the parser reads
//! the bytes as markup, and renaming it there would close a different element.
//! Such a name keeps its bytes when the attribute it would be carries no value,
//! which starts nothing: an empty `src` loads `about:blank`, an empty `srcdoc`
//! an empty document, and an empty `http-equiv` names no pragma. One followed
//! by `=`, after whitespace or none, carries a value and is renamed, so an end
//! tag with an `=` after its name, which is malformed markup, is the one place
//! the rest of the document can parse differently.
//!
//! Like the hint rewrite it reads the text without tracking parser context:
//! every `<iframe`, `<frame` or `<meta` followed by a tag-name terminator is
//! read as a tag, inside a comment, raw text or an attribute value included.
//! The attribute walks of all such candidates run together, one byte at a
//! time; walks that reach the same state at the same byte go on as one, so the
//! scan is linear in the document however the candidates overlap.

use std::borrow::Cow;

use super::attributes::{advance, is_html_space, tag_name_end, State};

/// What goes in front of a disarmed attribute's name. A `data-` attribute has
/// no meaning to an engine, and letters and hyphens keep every tokenizer state
/// the name passes through.
pub const DISARMED_PREFIX: &str = "data-writ-blocked-";

/// A walk's tag kind, as a bit of the mask a slot carries.
const FRAME: u8 = 1;
const META: u8 = 2;

/// The tags whose attributes start a navigation, and their kinds.
const CANDIDATES: [(&[u8], u8); 3] = [(b"iframe", FRAME), (b"frame", FRAME), (b"meta", META)];

const SRCDOC: &[u8] = b"srcdoc";
const HTTP_EQUIV: &[u8] = b"http-equiv";

/// The `http-equiv` values a document keeps, compared ASCII case-insensitively:
/// pragmas that set no navigation and open no connection.
pub const INERT_PRAGMAS: [&str; 5] = [
    "content-type",
    "content-language",
    "default-style",
    "x-ua-compatible",
    "content-security-policy",
];

/// The most bytes read past an `http-equiv` name for its value: room for the
/// longest inert pragma, its quotes and whitespace around the `=`. A value
/// that needs more is not kept.
const PRAGMA_LOOKAHEAD: usize = 64;

/// How much of a name the rewrite renames the current attribute name has
/// matched since it started: a prefix of `srcdoc` (which covers `src`) or of
/// `http-equiv`, or neither.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Matched {
    Other,
    Src(usize),
    Equiv(usize),
}

impl Matched {
    /// The match after a name's first byte.
    fn first(byte: u8) -> Self {
        match byte.to_ascii_lowercase() {
            b's' => Self::Src(1),
            b'h' => Self::Equiv(1),
            _ => Self::Other,
        }
    }

    /// The match after one more byte of the same name.
    fn then(self, byte: u8) -> Self {
        let byte = byte.to_ascii_lowercase();
        match self {
            Self::Src(len) if SRCDOC.get(len) == Some(&byte) => Self::Src(len + 1),
            Self::Equiv(len) if HTTP_EQUIV.get(len) == Some(&byte) => Self::Equiv(len + 1),
            _ => Self::Other,
        }
    }
}

/// The states a walk can be in, one slot each. A name's slot also records how
/// far it matches, so two walks share a slot only when their futures are the
/// same: the same bytes ahead, read from the same state, renaming the same
/// names.
const NAME_SLOTS: usize = 1 + SRCDOC.len() + HTTP_EQUIV.len();
const SLOTS: usize = 8 + NAME_SLOTS;
const BEFORE_NAME_SLOT: usize = 0;

fn slot_of(state: State, matched: Matched) -> usize {
    match state {
        State::BeforeName => BEFORE_NAME_SLOT,
        State::AfterName => 1,
        State::BeforeValue => 2,
        State::Quoted(b'"') => 3,
        State::Quoted(_) => 4,
        State::Unquoted => 5,
        State::AfterQuoted => 6,
        State::SelfClosing => 7,
        State::Name => match matched {
            Matched::Other => 8,
            Matched::Src(len) => 8 + len,
            Matched::Equiv(len) => 8 + SRCDOC.len() + len,
        },
    }
}

fn state_of(slot: usize) -> (State, Matched) {
    match slot {
        BEFORE_NAME_SLOT => (State::BeforeName, Matched::Other),
        1 => (State::AfterName, Matched::Other),
        2 => (State::BeforeValue, Matched::Other),
        3 => (State::Quoted(b'"'), Matched::Other),
        4 => (State::Quoted(b'\''), Matched::Other),
        5 => (State::Unquoted, Matched::Other),
        6 => (State::AfterQuoted, Matched::Other),
        7 => (State::SelfClosing, Matched::Other),
        8 => (State::Name, Matched::Other),
        _ if slot <= 8 + SRCDOC.len() => (State::Name, Matched::Src(slot - 8)),
        _ => (State::Name, Matched::Equiv(slot - 8 - SRCDOC.len())),
    }
}

/// Disarm every attribute that would start a frame navigation without a
/// script. Returns the input unchanged, without a copy, when it holds none.
pub fn disarm_frame_navigations(html: &str) -> Cow<'_, str> {
    let bytes = html.as_bytes();
    let mut walks = [0u8; SLOTS];
    let mut walking = false;
    // A candidate seen inside a walk, joining the walks where its name ends.
    let mut joining: Option<(usize, u8)> = None;
    let mut renamed: Vec<usize> = Vec::new();
    let mut at = 0;

    loop {
        if !walking {
            let Some((start, kind)) = joining.take().or_else(|| next_candidate(bytes, at)) else {
                break;
            };
            at = start;
            walks[BEFORE_NAME_SLOT] = kind;
        } else if let Some((start, kind)) = joining {
            if start == at {
                walks[BEFORE_NAME_SLOT] |= kind;
                joining = None;
            }
        }
        let Some(&byte) = bytes.get(at) else {
            // The text ended inside a tag, which is then never built.
            break;
        };
        if byte == b'<' {
            joining = candidate_at(bytes, at);
        }

        let mut next = [0u8; SLOTS];
        walking = false;
        for (slot, &kinds) in walks.iter().enumerate() {
            if kinds == 0 {
                continue;
            }
            let (state, matched) = state_of(slot);
            let step = advance(state, byte);
            if step.name_ends {
                if let Some(len) = renamed_name_len(matched, kinds, bytes, at) {
                    if !is_valueless_end_tag_name(bytes, at - len, at) {
                        renamed.push(at - len);
                    }
                }
            }
            if let Some(next_state) = step.next {
                let next_matched = match (next_state, step.name_starts) {
                    (State::Name, true) => Matched::first(byte),
                    (State::Name, false) => matched.then(byte),
                    _ => Matched::Other,
                };
                next[slot_of(next_state, next_matched)] |= kinds;
                walking = true;
            }
        }
        walks = next;
        at += 1;
    }

    if renamed.is_empty() {
        return Cow::Borrowed(html);
    }
    renamed.sort_unstable();
    renamed.dedup();
    let mut out = String::with_capacity(html.len() + renamed.len() * DISARMED_PREFIX.len());
    let mut copied = 0;
    for name_start in renamed {
        out.push_str(&html[copied..name_start]);
        out.push_str(DISARMED_PREFIX);
        copied = name_start;
    }
    out.push_str(&html[copied..]);
    Cow::Owned(out)
}

/// The candidate tag `<` at `open` starts, if any: where its name ends and its
/// kind.
fn candidate_at(bytes: &[u8], open: usize) -> Option<(usize, u8)> {
    CANDIDATES
        .iter()
        .find_map(|&(name, kind)| tag_name_end(bytes, open, name).map(|end| (end, kind)))
}

/// The next candidate tag at or after `from`.
fn next_candidate(bytes: &[u8], from: usize) -> Option<(usize, u8)> {
    let mut at = from;
    while at < bytes.len() {
        let open = at + bytes[at..].iter().position(|&b| b == b'<')?;
        if let Some(found) = candidate_at(bytes, open) {
            return Some(found);
        }
        at = open + 1;
    }
    None
}

/// The length of the name that ended just before `at`, when walks of `kinds`
/// rename it: `src` or `srcdoc` on a frame, `http-equiv` on a meta unless its
/// value is an inert pragma.
fn renamed_name_len(matched: Matched, kinds: u8, bytes: &[u8], at: usize) -> Option<usize> {
    match matched {
        Matched::Src(len) if (len == 3 || len == SRCDOC.len()) && kinds & FRAME != 0 => Some(len),
        Matched::Equiv(len)
            if len == HTTP_EQUIV.len() && kinds & META != 0 && !is_inert_pragma(bytes, at) =>
        {
            Some(len)
        }
        _ => None,
    }
}

/// Whether the name from `name_start` to `name_end` follows `</`, so that an
/// end tag reads it as its own name, and carries no value read as an attribute:
/// the name ends at `/` or `>`, or at whitespace that no `=` follows.
///
/// A name with a value is renamed in place, not left for a name inserted before
/// its `=`: in a tag that reads that `=` as the start of a name, the inserted
/// name would end at it and cut the value after it into attributes.
fn is_valueless_end_tag_name(bytes: &[u8], name_start: usize, name_end: usize) -> bool {
    if name_start < 2 || &bytes[name_start - 2..name_start] != b"</" {
        return false;
    }
    let mut at = name_end;
    while bytes.get(at).copied().is_some_and(is_html_space) {
        at += 1;
    }
    bytes.get(at) != Some(&b'=')
}

/// Whether the attribute whose name ends at `name_end` has, as its whole value
/// and in plain text, one of [`INERT_PRAGMAS`]. A missing or empty value, one
/// holding a character reference, padding or anything else, and one too far
/// ahead to read are not inert.
fn is_inert_pragma(bytes: &[u8], name_end: usize) -> bool {
    let limit = name_end.saturating_add(PRAGMA_LOOKAHEAD).min(bytes.len());
    let peek = |at: usize| (at < limit).then(|| bytes[at]);
    let mut at = name_end;
    while peek(at).is_some_and(is_html_space) {
        at += 1;
    }
    if peek(at) != Some(b'=') {
        return false;
    }
    at += 1;
    while peek(at).is_some_and(is_html_space) {
        at += 1;
    }
    let quote = match peek(at) {
        Some(quote @ (b'"' | b'\'')) => {
            at += 1;
            Some(quote)
        }
        Some(b'>') | None => return false,
        Some(_) => None,
    };
    let start = at;
    loop {
        match peek(at) {
            None => return false,
            Some(byte) if quote == Some(byte) => break,
            Some(byte) if quote.is_none() && (is_html_space(byte) || byte == b'>') => break,
            Some(_) => at += 1,
        }
    }
    let value = &bytes[start..at];
    INERT_PRAGMAS
        .iter()
        .any(|pragma| value.eq_ignore_ascii_case(pragma.as_bytes()))
}
