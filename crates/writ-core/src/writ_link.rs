//! `writ://` links: how another program points at a file in the notes folder,
//! or asks for a new one (ADR-052).
//!
//! Two verbs, each with the parameters it takes and nothing else:
//!
//! - `writ://open?path=<relative>&line=N` shows a file the notes folder holds,
//!   at a line when one is given.
//! - `writ://new?name=<name>` opens a new tab called `name`, or an untitled
//!   one with no name. The tab reaches a file on its first keystroke, as any
//!   new tab does (ADR-028 section 2), so a link writes nothing.
//!
//! A URL scheme is input from any web page or program on the machine, so the
//! grammar is an allowlist and [`parse`] refuses anything outside it rather
//! than reading past it: an unknown verb or parameter, a parameter given twice,
//! a fragment, a raw space or control character, an escape that is not two hex
//! digits, an absolute path, a backslash, a drive or a colon, and a `.`, `..`
//! or empty segment. Whether the file is inside the notes folder is asked
//! again where the folder is known, after the path is resolved on disk.
//!
//! Pure: the grammar and nothing else. `+` is a plus sign, not a space, and a
//! space is `%20`, so the links [`open_url`] builds read back unchanged.

use percent_encoding::{percent_decode_str, utf8_percent_encode, AsciiSet, NON_ALPHANUMERIC};

/// The scheme Writ answers for.
pub const SCHEME: &str = "writ";

/// The longest link [`parse`] reads, in bytes.
pub const MAX_LINK_BYTES: usize = 4096;

/// The longest name a `new` link may carry, in characters.
pub const MAX_NAME_CHARS: usize = 255;

/// The most digits a line number may have: `u32::MAX` has ten.
const MAX_LINE_DIGITS: usize = 10;

/// What a path segment keeps as it is in a built link: letters, digits,
/// `-._~` and the separator. Everything else is escaped, `+`, `&`, `=`, `#`,
/// `%` and spaces included.
const PATH_KEEPS: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'-')
    .remove(b'.')
    .remove(b'_')
    .remove(b'~')
    .remove(b'/');

/// What a `writ://` link asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WritLink {
    /// Show the file at `path`, relative to the notes folder with `/` between
    /// folders, at the 1-based `line` when there is one.
    Open {
        /// The path, decoded, with `/` between segments.
        path: String,
        /// The 1-based line to show.
        line: Option<u32>,
    },
    /// Open a new tab, called `name` when there is one. Nothing is written.
    New {
        /// The tab's name, decoded and trimmed.
        name: Option<String>,
    },
}

/// Why a link was refused. None of the messages repeats the link: a refusal is
/// logged, and the link may name a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum LinkRefusal {
    /// Over [`MAX_LINK_BYTES`].
    #[error("the link is longer than {MAX_LINK_BYTES} bytes")]
    TooLong,
    /// Another scheme.
    #[error("the link is not a writ:// link")]
    NotWrit,
    /// A space or a control character that was not percent-encoded.
    #[error("the link holds a space or a control character that is not encoded")]
    Unencoded,
    /// A part the grammar has no place for: a fragment, a missing `//`, or a
    /// parameter with no `=`.
    #[error("the link has a part writ:// links do not use")]
    Malformed,
    /// A verb other than `open` and `new`, or one with something after it.
    #[error("the link asks for something other than open or new")]
    UnknownVerb,
    /// A parameter the verb does not take.
    #[error("the link has a parameter its verb does not take")]
    UnknownParameter,
    /// One parameter given twice.
    #[error("the link gives one parameter twice")]
    RepeatedParameter,
    /// An escape that is not `%` and two hex digits, or bytes that are not
    /// UTF-8 once decoded.
    #[error("a parameter is not percent-encoded UTF-8")]
    BadEncoding,
    /// An `open` link with no path, or an empty one.
    #[error("the link names no file")]
    MissingPath,
    /// A path that is not relative to the notes folder: one starting with a
    /// separator, holding a backslash or a colon, or naming a drive.
    #[error("the path is not relative to the folder")]
    NotRelative,
    /// A segment that is empty, `.` or `..`, or holds a control character.
    #[error("the path has an empty, . or .. segment, or a control character")]
    BadSegment,
    /// A line that is not a number from 1 to `u32::MAX` in plain digits.
    #[error("the line is not a number from 1 up")]
    BadLine,
    /// A name that is empty, too long, `.` or `..`, or holds a separator, a
    /// colon or a control character.
    #[error("the name cannot be a file name")]
    BadName,
}

/// Reads `input` as a `writ://` link.
///
/// The scheme is matched without regard to case, as URL schemes are; the verb
/// and the parameter names exactly. `writ://open/?…` reads as `writ://open?…`,
/// because some handlers add the slash.
///
/// # Errors
///
/// [`LinkRefusal`]: the link is outside the grammar and asks for nothing.
pub fn parse(input: &str) -> Result<WritLink, LinkRefusal> {
    if input.len() > MAX_LINK_BYTES {
        return Err(LinkRefusal::TooLong);
    }
    if input.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(LinkRefusal::Unencoded);
    }
    let (scheme, rest) = input.split_once(':').ok_or(LinkRefusal::NotWrit)?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return Err(LinkRefusal::NotWrit);
    }
    let rest = rest.strip_prefix("//").ok_or(LinkRefusal::Malformed)?;
    if rest.contains('#') {
        return Err(LinkRefusal::Malformed);
    }
    let (verb, query) = rest.split_once('?').unwrap_or((rest, ""));
    let verb = verb.strip_suffix('/').unwrap_or(verb);
    let params = parameters(query)?;
    match verb {
        "open" => open_link(&params),
        "new" => new_link(&params),
        _ => Err(LinkRefusal::UnknownVerb),
    }
}

/// The link that opens `path`, relative to the notes folder with `/` between
/// folders, at `line` when there is one.
///
/// `None` for a path the grammar refuses, so every link handed out reads back
/// as the path and line it was built from. A line of 0 is left out.
pub fn open_url(path: &str, line: Option<u32>) -> Option<String> {
    let mut url = format!(
        "{SCHEME}://open?path={}",
        utf8_percent_encode(path, PATH_KEEPS)
    );
    if let Some(line) = line.filter(|line| *line > 0) {
        url.push_str(&format!("&line={line}"));
    }
    match parse(&url) {
        Ok(WritLink::Open { path: read, .. }) if read == path => Some(url),
        _ => None,
    }
}

/// Whether `text` is spelled as a `writ:` link, and so is read by [`parse`]
/// rather than as a path.
pub fn is_writ_link(text: &str) -> bool {
    text.get(..SCHEME.len() + 1)
        .is_some_and(|head| head.eq_ignore_ascii_case("writ:"))
}

/// The arguments a launch was given that are `writ:` links, in order.
///
/// Windows and Linux start the registered program with the link as an
/// argument; a path is never spelled `writ:`, and an argument that is not
/// UTF-8 is not a link.
pub fn link_args<I>(args: I) -> Vec<String>
where
    I: IntoIterator<Item = std::ffi::OsString>,
{
    args.into_iter()
        .filter_map(|arg| arg.into_string().ok())
        .filter(|arg| is_writ_link(arg))
        .collect()
}

/// One `key=value` pair of the query, the value still encoded.
struct Parameter<'a> {
    key: &'a str,
    value: &'a str,
}

/// The pairs of `query`, each key at most once.
fn parameters(query: &str) -> Result<Vec<Parameter<'_>>, LinkRefusal> {
    if query.is_empty() {
        return Ok(Vec::new());
    }
    let mut params: Vec<Parameter<'_>> = Vec::new();
    for piece in query.split('&') {
        let (key, value) = piece.split_once('=').ok_or(LinkRefusal::Malformed)?;
        if params.iter().any(|seen| seen.key == key) {
            return Err(LinkRefusal::RepeatedParameter);
        }
        params.push(Parameter { key, value });
    }
    Ok(params)
}

/// `writ://open`: a path, and a line or none.
fn open_link(params: &[Parameter<'_>]) -> Result<WritLink, LinkRefusal> {
    let mut path = None;
    let mut line = None;
    for param in params {
        match param.key {
            "path" => path = Some(relative_path(&decoded(param.value)?)?),
            "line" => line = Some(line_number(param.value)?),
            _ => return Err(LinkRefusal::UnknownParameter),
        }
    }
    Ok(WritLink::Open {
        path: path.ok_or(LinkRefusal::MissingPath)?,
        line,
    })
}

/// `writ://new`: a name or none.
fn new_link(params: &[Parameter<'_>]) -> Result<WritLink, LinkRefusal> {
    let mut name = None;
    for param in params {
        match param.key {
            "name" => name = Some(tab_name(&decoded(param.value)?)?),
            _ => return Err(LinkRefusal::UnknownParameter),
        }
    }
    Ok(WritLink::New { name })
}

/// `value` with its escapes decoded, refusing a `%` that is not followed by
/// two hex digits and bytes that are not UTF-8.
fn decoded(value: &str) -> Result<String, LinkRefusal> {
    let bytes = value.as_bytes();
    let mut at = 0;
    while let Some(offset) = bytes[at..].iter().position(|byte| *byte == b'%') {
        let escape = at + offset;
        let well_formed = bytes
            .get(escape + 1..escape + 3)
            .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit));
        if !well_formed {
            return Err(LinkRefusal::BadEncoding);
        }
        at = escape + 3;
    }
    percent_decode_str(value)
        .decode_utf8()
        .map(|text| text.into_owned())
        .map_err(|_| LinkRefusal::BadEncoding)
}

/// A decoded path, checked to stay relative to the folder it is joined to.
fn relative_path(path: &str) -> Result<String, LinkRefusal> {
    if path.is_empty() {
        return Err(LinkRefusal::MissingPath);
    }
    if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return Err(LinkRefusal::NotRelative);
    }
    for segment in path.split('/') {
        if segment.is_empty()
            || segment == "."
            || segment == ".."
            || segment.chars().any(char::is_control)
        {
            return Err(LinkRefusal::BadSegment);
        }
    }
    Ok(path.to_string())
}

/// A line number in plain digits, from 1 to `u32::MAX`.
fn line_number(value: &str) -> Result<u32, LinkRefusal> {
    if value.is_empty()
        || value.len() > MAX_LINE_DIGITS
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(LinkRefusal::BadLine);
    }
    value
        .parse::<u32>()
        .ok()
        .filter(|line| *line > 0)
        .ok_or(LinkRefusal::BadLine)
}

/// A decoded name, trimmed, checked to be one file name.
fn tab_name(name: &str) -> Result<String, LinkRefusal> {
    let name = name.trim();
    let refused = name.is_empty()
        || name == "."
        || name == ".."
        || name.chars().count() > MAX_NAME_CHARS
        || name
            .chars()
            .any(|c| c == '/' || c == '\\' || c == ':' || c.is_control());
    if refused {
        return Err(LinkRefusal::BadName);
    }
    Ok(name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seeded::Seeded;

    /// Pieces a link is built from, the hostile ones included, so a draw is
    /// mostly near the grammar rather than noise it refuses at the first
    /// character.
    const PIECES: &[&str] = &[
        "writ://",
        "WRIT://",
        "writ:",
        "open",
        "new",
        "/",
        "?",
        "&",
        "=",
        "path",
        "line",
        "name",
        "Plan.md",
        "a",
        "..",
        ".",
        "%2F",
        "%2e",
        "%2E%2E",
        "%00",
        "%0A",
        "%5C",
        "%3A",
        "%20",
        "+",
        "%",
        "%z",
        "%C3%A9",
        "%FF",
        "é",
        "#",
        ":",
        "C:",
        "\\",
        "0",
        "1",
        "12",
        "4294967296",
        "//",
    ];

    fn draw(seeded: &mut Seeded) -> String {
        let pieces = 1 + seeded.below(12);
        (0..pieces).map(|_| seeded.pick(PIECES)).collect()
    }

    fn assert_accepted_path_stays_inside(input: &str, path: &str) {
        assert!(!path.is_empty(), "{input:?}");
        assert!(!path.starts_with('/'), "{input:?} -> {path:?}");
        assert!(!path.contains('\\'), "{input:?} -> {path:?}");
        assert!(!path.contains(':'), "{input:?} -> {path:?}");
        assert!(!path.chars().any(char::is_control), "{input:?} -> {path:?}");
        for segment in path.split('/') {
            assert!(
                !segment.is_empty() && segment != "." && segment != "..",
                "{input:?} -> {path:?}"
            );
        }
    }

    #[test]
    fn no_drawn_link_is_accepted_with_a_path_that_leaves_the_folder() {
        for seed in 0..64 {
            let mut seeded = Seeded::new(seed);
            for _ in 0..500 {
                let input = draw(&mut seeded);
                match parse(&input) {
                    Ok(WritLink::Open { path, line }) => {
                        assert_accepted_path_stays_inside(&input, &path);
                        assert_ne!(line, Some(0), "{input:?}");
                    }
                    Ok(WritLink::New { name: Some(name) }) => {
                        assert!(!name.contains(['/', '\\', ':']), "{input:?} -> {name:?}");
                        assert!(!name.trim().is_empty(), "{input:?}");
                    }
                    Ok(WritLink::New { name: None }) | Err(_) => {}
                }
            }
        }
    }

    #[test]
    fn a_link_built_for_a_drawn_path_reads_back_as_that_path() {
        let segments = &[
            "Plan",
            "Plan.md",
            "C++ notes",
            "a&b=c",
            "100%",
            "#tag",
            "Café",
            "日本",
            "x?y",
            "~home",
            "a+b",
            "..x",
            "x..",
            " lead",
            "trail ",
        ];
        for seed in 0..32 {
            let mut seeded = Seeded::new(seed);
            for _ in 0..200 {
                let depth = 1 + seeded.below(4);
                let path = (0..depth)
                    .map(|_| seeded.pick(segments))
                    .collect::<Vec<_>>()
                    .join("/");
                let line = match seeded.below(3) {
                    0 => None,
                    _ => Some(1 + seeded.below(100_000)),
                };
                let url = open_url(&path, line).unwrap_or_else(|| panic!("{path:?}"));
                assert_eq!(parse(&url), Ok(WritLink::Open { path, line }), "{url}");
            }
        }
    }
}
