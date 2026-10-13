//! Resource hints in a previewed document, made inert before it is served.
//!
//! A `<link rel=preconnect>` (and `dns-prefetch`, `prefetch`, `preload`,
//! `prerender`, `modulepreload`) asks the engine to resolve a host or open a
//! connection before anything is fetched. WebKit and Chromium do that without
//! consulting the document's Content-Security-Policy, so `connect-src 'none'`
//! does not stop it: a downloaded document could make the engine contact a
//! host it names every time it is previewed (ADR-043). The preview has no use
//! for a `<link>` other than a stylesheet, so every other kind is disabled.
//!
//! A link is disabled by renaming its tag to `basefont`, attributes untouched.
//! The tree builder places a `basefont` start tag by every rule it uses for a
//! `link` (both are void and share each insertion mode's list), so the rest of
//! the document parses as it did; the element is hidden like a `link`, and no
//! engine fetches, resolves or connects for it. Renaming the element does not
//! depend on which `rel` a reader takes from a tag that carries several: the
//! tree builder keeps the first, while Chromium's speculative preload scanner
//! reads the raw token and acts on a later one.
//!
//! A link keeps its tag only when the tag ends, carries at most 64 attributes
//! and repeats no name, and its one `rel`, if it has one, names nothing but
//! `stylesheet` or `alternate` in plain text.
//!
//! It reads the text without tracking parser context: every `<link` followed
//! by a tag-name terminator is treated as a tag, including one inside a
//! comment, raw text or an attribute value. Wherever an engine could read a
//! link tag, the tag starts with those bytes and its attributes tokenize the
//! same way, so no context can hide one. Rewriting the text of an inert context
//! as well changes nothing an engine acts on. Attributes are read through the
//! tokenizer model in `attributes.rs`, which the frame rewrite in `frames.rs`
//! shares.

use std::borrow::Cow;

use super::attributes::{advance, is_html_space, tag_name_end, State};

/// What a disabled link's `<link` becomes.
const RENAMED_TAG: &str = "<basefont";

/// The rel keywords a link may keep: a stylesheet, whose fetch the CSP
/// governs, and an alternate, which fetches nothing by itself.
const KEPT_KEYWORDS: [&str; 2] = ["stylesheet", "alternate"];

/// The most attributes a kept link may carry. Each name is compared with the
/// earlier ones to find a repeat; a link with more is disabled unread.
const MAX_KEPT_ATTRIBUTES: usize = 64;

/// Bytes of attribute text read per byte of document before every remaining
/// candidate is disabled unread, so a document built from many unterminated
/// attributes cannot make the scan quadratic. Comparing attribute names draws
/// on the same budget.
const SCAN_BUDGET_PER_BYTE: usize = 8;
const SCAN_BUDGET_FLOOR: usize = 64 * 1024;

/// Disable every `<link>` that could ask the engine for anything but a
/// stylesheet. Returns the input unchanged, without a copy, when it holds no
/// such link.
pub fn neutralize_resource_hints(html: &str) -> Cow<'_, str> {
    let bytes = html.as_bytes();
    let mut budget = bytes
        .len()
        .saturating_mul(SCAN_BUDGET_PER_BYTE)
        .saturating_add(SCAN_BUDGET_FLOOR);
    let mut names = Vec::with_capacity(MAX_KEPT_ATTRIBUTES);
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut from = 0;

    while let Some((tag_open, after_name)) = next_link_candidate(bytes, from) {
        if !link_is_kept(bytes, after_name, &mut budget, &mut names) {
            let rewritten = out.get_or_insert_with(|| String::with_capacity(html.len() + 64));
            rewritten.push_str(&html[copied..tag_open]);
            rewritten.push_str(RENAMED_TAG);
            copied = after_name;
        }
        from = after_name;
    }

    match out {
        None => Cow::Borrowed(html),
        Some(mut rewritten) => {
            rewritten.push_str(&html[copied..]);
            Cow::Owned(rewritten)
        }
    }
}

/// The next `<` that opens a tag named `link`: the four letters in any case,
/// then whitespace, `/` or `>`. Returns the `<` and where the name ends.
fn next_link_candidate(bytes: &[u8], from: usize) -> Option<(usize, usize)> {
    let mut at = from;
    while at < bytes.len() {
        let open = at + bytes[at..].iter().position(|&b| b == b'<')?;
        if let Some(after_name) = tag_name_end(bytes, open, b"link") {
            return Some((open, after_name));
        }
        at = open + 1;
    }
    None
}

/// Read a tag's attributes from just after its name to the end of the tag and
/// decide whether the link may keep its tag. `names` holds the byte range of
/// each attribute name read so far. A tag that runs out of text or budget, or
/// that carries more than [`MAX_KEPT_ATTRIBUTES`], is not kept.
fn link_is_kept(
    bytes: &[u8],
    from: usize,
    budget: &mut usize,
    names: &mut Vec<(usize, usize)>,
) -> bool {
    names.clear();
    let mut state = State::BeforeName;
    let mut name_start = from;
    let mut current_is_rel = false;
    let mut value_start = from;

    for (at, &byte) in bytes.iter().enumerate().skip(from) {
        if *budget == 0 {
            return false;
        }
        *budget -= 1;
        let step = advance(state, byte);
        if step.name_ends {
            if !record_name(bytes, (name_start, at), names, budget) {
                return false;
            }
            current_is_rel = bytes[name_start..at].eq_ignore_ascii_case(b"rel");
        }
        if step.name_starts {
            name_start = at;
        }
        if let Some(offset) = step.value_starts {
            value_start = at + offset;
        }
        if step.value_ends && current_is_rel && !is_plain_and_kept(&bytes[value_start..at]) {
            return false;
        }
        match step.next {
            Some(next) => state = next,
            None => return true,
        }
    }
    // The text ended inside the tag.
    false
}

/// Add an attribute name to the tag's list. False when the name repeats an
/// earlier one (compared as the tokenizer compares them, ASCII case folded),
/// holds a NUL (which the tokenizer rewrites, so a byte comparison could miss a
/// repeat), or would take the tag past [`MAX_KEPT_ATTRIBUTES`].
fn record_name(
    bytes: &[u8],
    name: (usize, usize),
    names: &mut Vec<(usize, usize)>,
    budget: &mut usize,
) -> bool {
    let text = &bytes[name.0..name.1];
    if text.contains(&0) || names.len() == MAX_KEPT_ATTRIBUTES {
        return false;
    }
    for &(start, end) in names.iter() {
        let earlier = &bytes[start..end];
        if earlier.len() != text.len() {
            continue;
        }
        if *budget < text.len() {
            return false;
        }
        *budget -= text.len();
        if earlier.eq_ignore_ascii_case(text) {
            return false;
        }
    }
    names.push(name);
    true
}

/// Whether a rel value names only kept keywords in plain text: a value holding
/// a character reference or a NUL is never kept, since the bytes are not what
/// the tokenizer reads. An empty value names none and is kept.
fn is_plain_and_kept(value: &[u8]) -> bool {
    !value.iter().any(|&b| b == b'&' || b == 0)
        && value
            .split(|&b| is_html_space(b))
            .filter(|keyword| !keyword.is_empty())
            .all(|keyword| {
                KEPT_KEYWORDS
                    .iter()
                    .any(|kept| keyword.eq_ignore_ascii_case(kept.as_bytes()))
            })
}
