//! `writ-preview://` URL parser — pure-domain scope routing and refusal.
//!
//! The substrate decision (ADR-009 §A1, lean re-scope) makes Writ — not the
//! OS, not the renderer's heuristics — the boundary for the preview surface.
//! The chrome↔document scope boundary is enforced here: every incoming URL
//! is parsed into a [`ParsedRequest`] before any I/O, and cross-scope
//! traversal is refused.
//!
//! [`parse`] and [`split_asset_request`] are pure logic with no Tauri, no
//! I/O, and no allocation beyond the decoded path — so they live in
//! `writ-core` (per the crate-boundary rule) and can be fuzzed without
//! compiling the app shell. The debug-only disposition recorder that
//! observes the handler's decisions lives in `src-tauri`, which re-exports
//! these types.
//!
//! The note-asset half ([`resolve_asset`], [`resolve_asset_reference`])
//! resolves a reference against the filesystem, because containment is a
//! property of the resolved path and nothing else. It is read-only path
//! resolution: it opens no file, and the byte-serving side stays in
//! `src-tauri`. ADR-035.

use std::path::{Component, Path, PathBuf};

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

/// Why a request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// A note asset resolved outside every containment root.
    OutsideRoot,
    /// A note asset is a symbolic link. Links are refused rather than
    /// followed: the target is chosen by whoever wrote the link, not by the
    /// containment root.
    SymlinkRefused,
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
            Self::OutsideRoot => "outside_root",
            Self::SymlinkRefused => "symlink_refused",
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

// ─────────────────────────────── note assets ───────────────────────────────
//
// ADR-035. Images embedded in a note are served under the document scope at
// `_note-asset/<buffer id>/<root>/<relative path>`. The URL is a claim, not
// an authorization: every request is re-resolved against the same roots the
// render-time rewrite used, and containment is decided on the canonical
// path.

/// Reserved first segment of the note-asset route, under the document scope.
///
/// Deliberately distinct from `_assets`, which serves the bundled host
/// runtimes from the binary and has entirely different security properties.
pub const ASSET_PREFIX: &str = "_note-asset";

/// Folder inside the notes folder searched for an embedded file when the
/// reference does not resolve beside the note itself.
pub const ATTACHMENTS_DIR: &str = "attachments";

/// Which containment root an asset path is expressed relative to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssetRoot {
    /// The notes folder.
    Notes,
    /// The folder holding the note being previewed. Equal to, or inside,
    /// the notes folder for a note that lives there; the only root for a
    /// file opened from anywhere else.
    NoteDir,
}

impl AssetRoot {
    /// One-character URL discriminator.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Notes => "n",
            Self::NoteDir => "d",
        }
    }

    /// Parse the URL discriminator.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "n" => Some(Self::Notes),
            "d" => Some(Self::NoteDir),
            _ => None,
        }
    }
}

/// The three parts of an asset request, split out of a document-scope path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetRequest<'a> {
    /// Buffer whose render emitted the URL; keys the scope lookup.
    pub buffer_id: &'a str,
    /// Root the relative path is expressed against.
    pub root: AssetRoot,
    /// Path under that root. Already canonicalised by [`parse`]: no `..`,
    /// no backslash, no leading or repeated separator.
    pub relative: &'a str,
}

/// Split an already-parsed document-scope path into an asset request.
///
/// Returns `None` when the path is not on the asset route, when a part is
/// missing, or when the root discriminator is unknown. Pure: no I/O, no
/// panics on any input (the protocol fuzz target asserts this).
pub fn split_asset_request(document_path: &str) -> Option<AssetRequest<'_>> {
    let rest = document_path
        .strip_prefix(ASSET_PREFIX)?
        .strip_prefix('/')?;
    let (buffer_id, rest) = rest.split_once('/')?;
    let (root, relative) = rest.split_once('/')?;
    if buffer_id.is_empty() || relative.is_empty() {
        return None;
    }
    Some(AssetRequest {
        buffer_id,
        root: AssetRoot::from_token(root)?,
        relative,
    })
}

/// An embedded reference that resolved inside a containment root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssetReference {
    /// Root the reference resolved under.
    pub root: AssetRoot,
    /// Path relative to that root, `/`-separated and percent-encoded, ready
    /// to append to the asset URL.
    pub url_path: String,
    /// Absolute, canonical path on disk. May not exist: a reference to a
    /// missing file inside the root still resolves, so the preview can name
    /// it in a placeholder instead of showing a broken image.
    pub path: PathBuf,
}

/// Resolve an image reference written in a note to a containment-checked
/// path on disk.
///
/// `src` is the raw reference as authored — percent-encoded or not, relative
/// to the note, relative to the notes folder, or absolute. Candidates are
/// tried in that order plus [`ATTACHMENTS_DIR`], and the first that exists
/// wins; when none exists the first contained candidate is returned so the
/// caller can name the missing file.
///
/// Refuses anything that resolves outside both roots, and refuses a symbolic
/// link outright rather than following it.
pub fn resolve_asset_reference(
    notes_root: &Path,
    note_dir: &Path,
    src: &str,
) -> Result<AssetReference, RefusalReason> {
    let decoded = percent_decode(src).unwrap_or_else(|_| src.to_string());
    let decoded = decoded.replace('\\', "/");
    let trimmed = decoded.trim();
    if trimmed.is_empty() {
        return Err(RefusalReason::MalformedUrl);
    }
    if trimmed.chars().any(|c| (c as u32) < 0x20 || c == '\x7f') {
        return Err(RefusalReason::ProhibitedCharacter);
    }

    let notes_canonical = canonical_root(notes_root);
    let dir_canonical = canonical_root(note_dir);

    let mut candidates: Vec<PathBuf> = Vec::with_capacity(4);
    match trimmed.strip_prefix('/') {
        // A leading separator means "from the notes folder", the spelling
        // Obsidian uses. The same text is also a filesystem-absolute path,
        // so both readings are offered — both containment-checked.
        Some(from_notes_root) => {
            let from_notes_root = from_notes_root.trim_start_matches('/');
            if !from_notes_root.is_empty() {
                candidates.push(notes_root.join(from_notes_root));
            }
            candidates.push(PathBuf::from(trimmed));
        }
        None => {
            candidates.push(note_dir.join(trimmed));
            candidates.push(notes_root.join(trimmed));
            candidates.push(notes_root.join(ATTACHMENTS_DIR).join(trimmed));
        }
    }

    let mut fallback: Option<AssetReference> = None;
    let mut first_error: Option<RefusalReason> = None;
    for candidate in candidates {
        match contain(
            notes_canonical.as_deref(),
            dir_canonical.as_deref(),
            &candidate,
        ) {
            // A link is refused wherever it is found: following it would let
            // the note's author, not the root, choose the served file.
            Err(RefusalReason::SymlinkRefused) => return Err(RefusalReason::SymlinkRefused),
            Err(reason) => {
                first_error.get_or_insert(reason);
            }
            Ok(reference) => {
                if reference.path.is_file() {
                    return Ok(reference);
                }
                fallback.get_or_insert(reference);
            }
        }
    }

    fallback.ok_or(first_error.unwrap_or(RefusalReason::OutsideRoot))
}

/// Re-resolve an asset request at serve time.
///
/// The URL carries a root and a relative path; neither is trusted. The path
/// is joined onto the named root and put through the same canonicalisation
/// and containment check the render-time rewrite used.
pub fn resolve_asset(
    notes_root: &Path,
    note_dir: &Path,
    request: AssetRequest<'_>,
) -> Result<PathBuf, RefusalReason> {
    let root = match request.root {
        AssetRoot::Notes => notes_root,
        AssetRoot::NoteDir => note_dir,
    };
    let relative = Path::new(request.relative);
    if relative
        .components()
        .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err(RefusalReason::TraversalAttempt);
    }
    let canonical = canonical_root(root).ok_or(RefusalReason::OutsideRoot)?;
    contain(Some(&canonical), None, &root.join(relative)).map(|reference| reference.path)
}

/// Canonicalise a containment root so both sides of the `starts_with` check
/// agree. Mirrors `security::authorized_paths::canonicalize_root`, which
/// cannot be reused here: it lives in `src-tauri`.
fn canonical_root(path: &Path) -> Option<PathBuf> {
    std::fs::canonicalize(path).ok().map(strip_unc_prefix)
}

/// Resolve `candidate` and place it under whichever root contains it.
///
/// The notes folder is tried first, so a note that lives inside it gets URLs
/// that stay valid when the note moves within the folder.
fn contain(
    notes_root: Option<&Path>,
    note_dir: Option<&Path>,
    candidate: &Path,
) -> Result<AssetReference, RefusalReason> {
    if std::fs::symlink_metadata(candidate).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(RefusalReason::SymlinkRefused);
    }
    let resolved = resolve_existing_prefix(candidate).ok_or(RefusalReason::OutsideRoot)?;
    for (root, path) in [
        (AssetRoot::Notes, notes_root),
        (AssetRoot::NoteDir, note_dir),
    ] {
        let Some(path) = path else { continue };
        if let Ok(relative) = resolved.strip_prefix(path) {
            let url_path = encode_relative(relative).ok_or(RefusalReason::InvalidEncoding)?;
            if url_path.is_empty() {
                continue;
            }
            return Ok(AssetReference {
                root,
                url_path,
                path: resolved,
            });
        }
    }
    Err(RefusalReason::OutsideRoot)
}

/// Canonicalise as much of `path` as exists, then append the components that
/// do not. Every symlink and every `..` above the missing tail is resolved by
/// the filesystem; only names it has never seen are appended literally, so a
/// reference to a file that is not there yet is still judged honestly.
fn resolve_existing_prefix(path: &Path) -> Option<PathBuf> {
    let mut unresolved: Vec<std::ffi::OsString> = Vec::new();
    let mut cursor = path.to_path_buf();
    loop {
        match std::fs::canonicalize(&cursor) {
            Ok(base) => {
                let mut resolved = strip_unc_prefix(base);
                for name in unresolved.iter().rev() {
                    resolved.push(name);
                }
                return Some(resolved);
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                unresolved.push(cursor.file_name()?.to_os_string());
                cursor = cursor.parent()?.to_path_buf();
            }
            Err(_) => return None,
        }
    }
}

/// Percent-encode a relative path into the `/`-separated form the URL
/// carries. Returns `None` for a path with a non-`Normal` component or a
/// non-UTF-8 name.
fn encode_relative(relative: &Path) -> Option<String> {
    let mut out = String::new();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            return None;
        };
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(&percent_encode_segment(name.to_str()?));
    }
    Some(out)
}

/// Percent-encode one path segment, leaving only the URL-unreserved set.
fn percent_encode_segment(segment: &str) -> String {
    let mut out = String::with_capacity(segment.len());
    for byte in segment.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(*byte as char)
            }
            other => out.push_str(&format!("%{other:02X}")),
        }
    }
    out
}

#[cfg(windows)]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    const UNC: &str = r"\\?\";
    match path.to_str() {
        Some(s) if s.starts_with(UNC) => PathBuf::from(&s[UNC.len()..]),
        _ => path,
    }
}

#[cfg(not(windows))]
fn strip_unc_prefix(path: PathBuf) -> PathBuf {
    path
}

/// MIME type of an image, decided by the leading bytes rather than the file
/// name. A `.png` holding markup is served as nothing at all: the extension
/// is what an attacker controls, the magic number is not.
///
/// Returns `None` for anything that is not one of the image formats the
/// preview serves, which the caller turns into a placeholder.
pub fn sniff_image_mime(bytes: &[u8]) -> Option<&'static str> {
    const PNG: &[u8] = b"\x89PNG\r\n\x1a\n";
    if bytes.starts_with(PNG) {
        return Some("image/png");
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return Some("image/jpeg");
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some("image/gif");
    }
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    if bytes.starts_with(b"BM") {
        return Some("image/bmp");
    }
    if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" && &bytes[8..11] == b"avi" {
        return Some("image/avif");
    }
    if is_svg(bytes) {
        return Some("image/svg+xml");
    }
    None
}

/// True when the first element of an XML document is `<svg>`.
///
/// The whole prefix is examined, not searched: markup that opens with an
/// HTML doctype or an `<html>` element is not an SVG no matter what appears
/// further down, which is what keeps an HTML file named `.png` from being
/// served as an image.
fn is_svg(bytes: &[u8]) -> bool {
    let prefix = &bytes[..bytes.len().min(1024)];
    // A multi-byte character truncated by the window edge is not a decoding
    // failure of the document, so the largest valid slice is examined.
    let text = match std::str::from_utf8(prefix) {
        Ok(text) => text,
        Err(err) => match std::str::from_utf8(&prefix[..err.valid_up_to()]) {
            Ok(text) => text,
            Err(_) => return false,
        },
    };
    svg_element_is_first(text)
}

fn svg_element_is_first(text: &str) -> bool {
    let mut rest = text.trim_start_matches('\u{feff}').trim_start();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("<?") {
            // XML declaration or processing instruction.
            let Some((_, tail)) = after.split_once("?>") else {
                return false;
            };
            rest = tail;
            continue;
        }
        if let Some(after) = rest.strip_prefix("<!--") {
            let Some((_, tail)) = after.split_once("-->") else {
                return false;
            };
            rest = tail;
            continue;
        }
        if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("<!doctype") {
            // Only an `svg` doctype may precede an SVG root element.
            let Some((declaration, tail)) = rest.split_once('>') else {
                return false;
            };
            if !declaration[9..]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("svg")
            {
                return false;
            }
            rest = tail;
            continue;
        }
        let Some(after) = rest.strip_prefix('<') else {
            return false;
        };
        let name_end = after
            .find(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .unwrap_or(after.len());
        return after[..name_end].eq_ignore_ascii_case("svg");
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
        assert_eq!(
            parse("writ-workspace://chrome/x"),
            Err(RefusalReason::WrongScheme)
        );
    }

    #[test]
    fn rejects_unknown_scope() {
        assert_eq!(
            parse("writ-preview://attacker/x"),
            Err(RefusalReason::UnknownScope)
        );
        assert_eq!(parse("writ-preview:///x"), Err(RefusalReason::UnknownScope));
    }

    #[test]
    fn rejects_dot_dot_traversal() {
        for url in [
            "writ-preview://document/../chrome/base.css",
            "writ-preview://document/buf-1/../../chrome/base.css",
            "writ-preview://chrome/../document/x",
        ] {
            assert_eq!(
                parse(url),
                Err(RefusalReason::TraversalAttempt),
                "url={url}"
            );
        }
    }

    #[test]
    fn rejects_percent_encoded_traversal() {
        for url in [
            "writ-preview://document/%2e%2e/chrome/base.css",
            "writ-preview://document/%2E%2E/chrome/base.css",
            "writ-preview://document/foo/%2e%2e/bar",
        ] {
            assert_eq!(
                parse(url),
                Err(RefusalReason::TraversalAttempt),
                "url={url}"
            );
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

#[cfg(test)]
mod asset_tests {
    use super::*;

    /// A notes folder with a note in `daily/`, an image beside it, and one
    /// under the notes-folder attachments folder.
    fn fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("Writ");
        let note_dir = notes.join("daily");
        std::fs::create_dir_all(&note_dir).unwrap();
        std::fs::create_dir_all(notes.join(ATTACHMENTS_DIR)).unwrap();
        std::fs::write(note_dir.join("beside.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(
            notes.join(ATTACHMENTS_DIR).join("a.png"),
            b"\x89PNG\r\n\x1a\n",
        )
        .unwrap();
        std::fs::write(notes.join("root.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        (dir, notes, note_dir)
    }

    fn resolve(notes: &Path, note_dir: &Path, src: &str) -> Result<AssetReference, RefusalReason> {
        resolve_asset_reference(notes, note_dir, src)
    }

    #[test]
    fn resolves_a_sibling_file_against_the_note_folder() {
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "beside.png").unwrap();
        assert_eq!(r.root, AssetRoot::Notes);
        assert_eq!(r.url_path, "daily/beside.png");
        assert!(r.path.ends_with("daily/beside.png"));
    }

    #[test]
    fn resolves_through_the_attachments_folder() {
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "a.png").unwrap();
        assert_eq!(r.url_path, "attachments/a.png");
    }

    #[test]
    fn resolves_a_notes_folder_absolute_reference() {
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "/root.png").unwrap();
        assert_eq!(r.url_path, "root.png");
    }

    #[test]
    fn resolves_a_relative_reference_that_climbs_inside_the_notes_folder() {
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "../root.png").unwrap();
        assert_eq!(r.url_path, "root.png");
    }

    #[test]
    fn percent_encoded_and_spaced_names_round_trip() {
        let (_g, notes, note_dir) = fixture();
        std::fs::write(note_dir.join("my shot.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        for src in ["my shot.png", "my%20shot.png"] {
            let r = resolve(&notes, &note_dir, src).unwrap();
            assert_eq!(r.url_path, "daily/my%20shot.png", "src={src}");
            let url = format!(
                "writ-preview://document/{ASSET_PREFIX}/buf-1/{}/{}",
                r.root.as_str(),
                r.url_path
            );
            let parsed = parse(&url).unwrap();
            let request = split_asset_request(&parsed.path).unwrap();
            assert_eq!(resolve_asset(&notes, &note_dir, request).unwrap(), r.path);
        }
    }

    #[test]
    fn refuses_a_reference_that_climbs_out_of_every_root() {
        let (_g, notes, note_dir) = fixture();
        assert_eq!(
            resolve(&notes, &note_dir, "../../../etc/hosts"),
            Err(RefusalReason::OutsideRoot)
        );
    }

    #[test]
    fn a_leading_separator_reads_from_the_notes_folder_not_the_filesystem() {
        // `/etc/hosts` is the notes-folder-relative spelling, so it resolves
        // to a path that does not exist inside the folder — never to the
        // real file. Nothing outside the roots is reachable either way.
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "/etc/hosts").unwrap();
        assert_eq!(r.url_path, "etc/hosts");
        assert!(r.path.starts_with(std::fs::canonicalize(&notes).unwrap()));
        assert!(!r.path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_rather_than_following_it() {
        let (guard, notes, note_dir) = fixture();
        let outside = guard.path().join("outside.png");
        std::fs::write(&outside, b"\x89PNG\r\n\x1a\n").unwrap();
        std::os::unix::fs::symlink(&outside, note_dir.join("link.png")).unwrap();
        assert_eq!(
            resolve(&notes, &note_dir, "link.png"),
            Err(RefusalReason::SymlinkRefused)
        );
        // A link that stays inside the root is refused on the same rule: the
        // target is chosen by the link, not by the containment check.
        std::os::unix::fs::symlink(notes.join("root.png"), note_dir.join("inner.png")).unwrap();
        assert_eq!(
            resolve(&notes, &note_dir, "inner.png"),
            Err(RefusalReason::SymlinkRefused)
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_file_reached_through_a_symlinked_folder() {
        let (guard, notes, note_dir) = fixture();
        let outside = guard.path().join("elsewhere");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("x.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        std::os::unix::fs::symlink(&outside, note_dir.join("shots")).unwrap();
        // The linked folder is never followed out of the root. What survives
        // is a path inside the notes folder that does not exist, which the
        // preview names in a placeholder — the file outside is unreachable
        // either way.
        match resolve(&notes, &note_dir, "shots/x.png") {
            Err(RefusalReason::OutsideRoot) => {}
            Ok(reference) => {
                assert!(reference
                    .path
                    .starts_with(std::fs::canonicalize(&notes).unwrap()));
                assert!(!reference.path.exists());
            }
            other => panic!("unexpected resolution: {other:?}"),
        }
    }

    #[test]
    fn a_missing_file_inside_the_root_still_resolves_so_it_can_be_named() {
        let (_g, notes, note_dir) = fixture();
        let r = resolve(&notes, &note_dir, "gone.png").unwrap();
        assert_eq!(r.url_path, "daily/gone.png");
        assert!(!r.path.exists());
    }

    #[test]
    fn a_file_outside_the_notes_folder_resolves_only_under_its_own_folder() {
        let dir = tempfile::tempdir().unwrap();
        let notes = dir.path().join("Writ");
        let elsewhere = dir.path().join("elsewhere");
        std::fs::create_dir_all(&notes).unwrap();
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("a.png"), b"\x89PNG\r\n\x1a\n").unwrap();
        let r = resolve(&notes, &elsewhere, "a.png").unwrap();
        assert_eq!(r.root, AssetRoot::NoteDir);
        assert_eq!(r.url_path, "a.png");
    }

    #[test]
    fn serve_time_resolution_refuses_a_traversal_relative_path() {
        let (_g, notes, note_dir) = fixture();
        let request = AssetRequest {
            buffer_id: "b",
            root: AssetRoot::Notes,
            relative: "../root.png",
        };
        assert_eq!(
            resolve_asset(&notes, &note_dir, request),
            Err(RefusalReason::TraversalAttempt)
        );
    }

    #[cfg(unix)]
    #[test]
    fn serve_time_resolution_refuses_a_symlink() {
        let (_g, notes, note_dir) = fixture();
        std::os::unix::fs::symlink(notes.join("root.png"), notes.join("link.png")).unwrap();
        let request = AssetRequest {
            buffer_id: "b",
            root: AssetRoot::Notes,
            relative: "link.png",
        };
        assert_eq!(
            resolve_asset(&notes, &note_dir, request),
            Err(RefusalReason::SymlinkRefused)
        );
    }

    #[test]
    fn splits_a_well_formed_asset_request() {
        let req = parse(&format!(
            "writ-preview://document/{ASSET_PREFIX}/buf-1/n/attachments/a.png"
        ))
        .unwrap();
        let asset = split_asset_request(&req.path).unwrap();
        assert_eq!(asset.buffer_id, "buf-1");
        assert_eq!(asset.root, AssetRoot::Notes);
        assert_eq!(asset.relative, "attachments/a.png");
    }

    #[test]
    fn split_rejects_malformed_or_unrelated_paths() {
        for path in [
            "buf-1/index.html",
            "_assets/mermaid/mermaid.min.js",
            "_note-asset",
            "_note-asset/buf-1",
            "_note-asset/buf-1/n",
            "_note-asset/buf-1/x/a.png",
            "_note-assets/buf-1/n/a.png",
        ] {
            assert!(split_asset_request(path).is_none(), "path={path}");
        }
    }

    #[test]
    fn sniffs_the_image_formats_the_preview_serves() {
        assert_eq!(sniff_image_mime(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(
            sniff_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(sniff_image_mime(b"GIF89a....."), Some("image/gif"));
        assert_eq!(
            sniff_image_mime(b"RIFF\0\0\0\0WEBPVP8 "),
            Some("image/webp")
        );
        assert_eq!(sniff_image_mime(b"BM......"), Some("image/bmp"));
        assert_eq!(
            sniff_image_mime(b"<svg xmlns=\"http://www.w3.org/2000/svg\"></svg>"),
            Some("image/svg+xml")
        );
        assert_eq!(
            sniff_image_mime(b"<?xml version=\"1.0\"?>\n<!-- c -->\n<svg/>"),
            Some("image/svg+xml")
        );
    }

    #[test]
    fn markup_that_is_not_an_svg_sniffs_as_nothing() {
        // The fixture that matters: a `.png` holding HTML. The extension is
        // attacker-controlled; the leading bytes are not.
        for bytes in [
            &b"<!doctype html><html><body><svg></svg></body></html>"[..],
            &b"<html><script>x()</script></html>"[..],
            &b"\n  <body><svg/></body>"[..],
            &b"not an image at all"[..],
            &b""[..],
        ] {
            assert_eq!(sniff_image_mime(bytes), None);
        }
    }
}
