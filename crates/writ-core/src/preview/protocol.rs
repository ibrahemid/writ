//! `writ-preview://` URL parser — pure-domain scope routing and refusal.
//!
//! The substrate decision (ADR-009 §A1, lean re-scope) makes Writ — not the
//! OS, not the renderer's heuristics — the boundary for the preview surface.
//! The chrome↔document scope boundary is enforced here: every incoming URL
//! is parsed into a [`ParsedRequest`] before any I/O, and cross-scope
//! traversal is refused.
//!
//! This is pure logic with no Tauri, no I/O, and no allocation beyond the
//! decoded path — so it lives in `writ-core` (per the crate-boundary rule)
//! and can be fuzzed without compiling the app shell. The debug-only
//! disposition recorder that observes the handler's decisions lives in
//! `src-tauri`, which re-exports these types.

/// Scope side of the `writ-preview://` split — ADR-009 §A1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewScope {
    /// Bundled trusted assets the host owns (fallback stylesheet, and —
    /// from L5/L6 — the Mermaid and KaTeX runtimes).
    Chrome,
    /// User-authored document bytes served under the fixed document CSP.
    Document,
}

impl PreviewScope {
    /// Human-readable name (used for diagnostics and the disposition log).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Chrome => "chrome",
            Self::Document => "document",
        }
    }
}

/// Successfully parsed incoming request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequest {
    /// Side of the chrome/document split.
    pub scope: PreviewScope,
    /// Path within the scope. Already canonicalised: no leading or
    /// repeated slashes, no traversal segments, percent-decoded.
    pub path: String,
}

/// Why a request was refused before any I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefusalReason {
    /// URL scheme is not `writ-preview`.
    WrongScheme,
    /// Host segment is neither `chrome` nor `document`.
    UnknownScope,
    /// Path contained `..`, an absolute prefix, or a similar traversal
    /// attempt.
    TraversalAttempt,
    /// Path contained a null byte or other prohibited control character.
    ProhibitedCharacter,
    /// Path could not be parsed as valid percent-encoded UTF-8.
    InvalidEncoding,
    /// URL was empty or otherwise un-parseable as a URL.
    MalformedUrl,
}

impl RefusalReason {
    /// Stable identifier suitable for logging and disposition records.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WrongScheme => "wrong_scheme",
            Self::UnknownScope => "unknown_scope",
            Self::TraversalAttempt => "traversal_attempt",
            Self::ProhibitedCharacter => "prohibited_character",
            Self::InvalidEncoding => "invalid_encoding",
            Self::MalformedUrl => "malformed_url",
        }
    }
}

/// Parse a `writ-preview://` URL into a [`ParsedRequest`] or a
/// [`RefusalReason`].
///
/// Pure: no I/O, no panics on any input (the fuzz target asserts this).
pub fn parse(url: &str) -> Result<ParsedRequest, RefusalReason> {
    if url.is_empty() {
        return Err(RefusalReason::MalformedUrl);
    }

    let scheme_end = url.find("://").ok_or(RefusalReason::MalformedUrl)?;
    let scheme = &url[..scheme_end];
    if !scheme.eq_ignore_ascii_case("writ-preview") {
        return Err(RefusalReason::WrongScheme);
    }

    let rest = &url[scheme_end + 3..];
    // Strip query and fragment — neither carries authorization meaning;
    // both are discarded before path validation.
    let rest = rest.split_once(['?', '#']).map(|(p, _)| p).unwrap_or(rest);

    let (host, raw_path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx + 1..]),
        None => (rest, ""),
    };

    let scope = match host {
        "chrome" => PreviewScope::Chrome,
        "document" => PreviewScope::Document,
        _ => return Err(RefusalReason::UnknownScope),
    };

    let canonical = canonicalize_path(raw_path)?;
    Ok(ParsedRequest {
        scope,
        path: canonical,
    })
}

/// Decode percent-encoded UTF-8, reject prohibited characters, reject
/// traversal, normalise repeated slashes.
///
/// The `writ-preview://` path is a key under a scope, not a filesystem
/// path: there is nothing to escape "out of" except the chrome↔document
/// boundary, which the segment-walker catches via the explicit `..`
/// rejection. Leading and repeated slashes are normalised away the same
/// way browsers normalise URL paths.
fn canonicalize_path(raw: &str) -> Result<String, RefusalReason> {
    let decoded = percent_decode(raw)?;

    // Reject null bytes and other ASCII control characters anywhere in
    // the path. The webview's parser is permissive about these; we are not.
    if decoded.chars().any(|c| (c as u32) < 0x20 || c == '\x7f') {
        return Err(RefusalReason::ProhibitedCharacter);
    }

    // Backslashes are normalised to forward slashes so the Windows-style
    // traversal `\..\` is caught by the same segment check the POSIX-style
    // `/../` is.
    let normalised = decoded.replace('\\', "/");

    let mut canonical_segments: Vec<&str> = Vec::new();
    for segment in normalised.split('/') {
        match segment {
            "" | "." => continue,
            ".." => return Err(RefusalReason::TraversalAttempt),
            other => canonical_segments.push(other),
        }
    }

    Ok(canonical_segments.join("/"))
}

fn percent_decode(input: &str) -> Result<String, RefusalReason> {
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' {
            if i + 2 >= bytes.len() {
                return Err(RefusalReason::InvalidEncoding);
            }
            let hi = hex_value(bytes[i + 1]).ok_or(RefusalReason::InvalidEncoding)?;
            let lo = hex_value(bytes[i + 2]).ok_or(RefusalReason::InvalidEncoding)?;
            out.push((hi << 4) | lo);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).map_err(|_| RefusalReason::InvalidEncoding)
}

fn hex_value(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chrome_root() {
        let req = parse("writ-preview://chrome/preview-base.css").unwrap();
        assert_eq!(req.scope, PreviewScope::Chrome);
        assert_eq!(req.path, "preview-base.css");
    }

    #[test]
    fn parses_document_with_nested_path() {
        let req = parse("writ-preview://document/buf-1/index.html").unwrap();
        assert_eq!(req.scope, PreviewScope::Document);
        assert_eq!(req.path, "buf-1/index.html");
    }

    #[test]
    fn normalises_repeated_slashes() {
        let req = parse("writ-preview://chrome///nested//asset.css").unwrap();
        assert_eq!(req.path, "nested/asset.css");
    }

    #[test]
    fn discards_query_and_fragment() {
        let req = parse("writ-preview://document/buf-1?cache=bust#hash").unwrap();
        assert_eq!(req.path, "buf-1");
    }

    #[test]
    fn empty_path_yields_empty_string() {
        let req = parse("writ-preview://chrome/").unwrap();
        assert_eq!(req.path, "");
        let req = parse("writ-preview://chrome").unwrap();
        assert_eq!(req.path, "");
    }

    #[test]
    fn case_insensitive_scheme() {
        let req = parse("WRIT-PREVIEW://chrome/x").unwrap();
        assert_eq!(req.scope, PreviewScope::Chrome);
    }

    #[test]
    fn rejects_wrong_scheme() {
        assert_eq!(parse("https://chrome/x"), Err(RefusalReason::WrongScheme));
        assert_eq!(parse("writ-workspace://chrome/x"), Err(RefusalReason::WrongScheme));
    }

    #[test]
    fn rejects_unknown_scope() {
        assert_eq!(parse("writ-preview://attacker/x"), Err(RefusalReason::UnknownScope));
        assert_eq!(parse("writ-preview:///x"), Err(RefusalReason::UnknownScope));
    }

    #[test]
    fn rejects_dot_dot_traversal() {
        for url in [
            "writ-preview://document/../chrome/base.css",
            "writ-preview://document/buf-1/../../chrome/base.css",
            "writ-preview://chrome/../document/x",
        ] {
            assert_eq!(parse(url), Err(RefusalReason::TraversalAttempt), "url={url}");
        }
    }

    #[test]
    fn rejects_percent_encoded_traversal() {
        for url in [
            "writ-preview://document/%2e%2e/chrome/base.css",
            "writ-preview://document/%2E%2E/chrome/base.css",
            "writ-preview://document/foo/%2e%2e/bar",
        ] {
            assert_eq!(parse(url), Err(RefusalReason::TraversalAttempt), "url={url}");
        }
    }

    #[test]
    fn double_encoded_traversal_decodes_once_and_does_not_collapse() {
        // The handler percent-decodes exactly once. A double-encoded
        // sequence (`%252e%252e`) decodes to the literal text `%2e%2e`,
        // which is not a `..` segment, so it passes through as an ordinary
        // (nonexistent) key rather than being treated as traversal. The
        // single-encoded form is what an attacker would have to use, and
        // that IS rejected — see `rejects_percent_encoded_traversal`.
        let req = parse("writ-preview://document/%252e%252e/x").unwrap();
        assert_eq!(req.path, "%2e%2e/x");
    }

    #[test]
    fn rejects_backslash_traversal_on_windows_style_paths() {
        assert_eq!(
            parse("writ-preview://document/..\\chrome\\base.css"),
            Err(RefusalReason::TraversalAttempt),
        );
        assert_eq!(
            parse("writ-preview://document/foo\\..\\bar"),
            Err(RefusalReason::TraversalAttempt),
        );
    }

    #[test]
    fn rejects_null_byte_in_path() {
        assert_eq!(
            parse("writ-preview://document/foo%00bar"),
            Err(RefusalReason::ProhibitedCharacter),
        );
    }

    #[test]
    fn normalises_leading_doubled_separator_into_key() {
        // `writ-preview://` paths are scope-prefixed keys, not filesystem
        // paths. The host↔path separator and any repeated separators are
        // normalised away. The chrome↔document boundary is the only crossing
        // that matters and is enforced by the `..` segment rejection, not by
        // string-prefix sniffing.
        let req = parse("writ-preview://document//etc/passwd").unwrap();
        assert_eq!(req.path, "etc/passwd");
    }

    #[test]
    fn windows_drive_prefix_is_just_a_path_segment() {
        // Same reasoning: a leading `C:` is a literal key character, not a
        // Windows drive prefix in the preview protocol's semantics. The
        // chrome-scope asset table simply does not contain such a key and
        // the request 404s downstream.
        let req = parse("writ-preview://document/C:%2Fwindows").unwrap();
        assert_eq!(req.path, "C:/windows");
    }

    #[test]
    fn rejects_invalid_percent_encoding() {
        assert_eq!(
            parse("writ-preview://document/foo%2"),
            Err(RefusalReason::InvalidEncoding),
        );
        assert_eq!(
            parse("writ-preview://document/foo%xx"),
            Err(RefusalReason::InvalidEncoding),
        );
    }

    #[test]
    fn rejects_empty_url() {
        assert_eq!(parse(""), Err(RefusalReason::MalformedUrl));
        assert_eq!(parse("writ-preview"), Err(RefusalReason::MalformedUrl));
    }
}
