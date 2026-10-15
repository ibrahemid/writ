//! One Writ per data folder (ADR-033, amended 2026-10-14).
//!
//! The process holding `<data dir>/instance.lock` is the only one that opens
//! the database and writes notes. A second launch on the same folder hands the
//! paths it was given to that process as an [`OpenRequest`] and exits.
//!
//! The `writ` command leaves the same request when a path carries a line to
//! open at, or when it waits for the tab to close (`writ --wait`, ADR-017),
//! and `writ mcp` leaves one carrying a `writ://` link (ADR-052) or asking
//! which file is open (ADR-053).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// How long a request stays worth acting on, in milliseconds.
///
/// A request the running instance never read (it was quitting as the second
/// launch wrote it) is found by the next launch's first look. Opening it then
/// is right a moment later and wrong a day later.
pub const OPEN_REQUEST_FRESH_MS: i64 = 60_000;

/// Longest token a waiting `writ` names itself by.
pub const MAX_WAIT_TOKEN_LEN: usize = 64;

/// What a second launch asks the running instance to do: open `paths` and
/// follow `links`, answer a `context` ask, or with none of them, come
/// forward.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// The paths as the second launch was given them; the receiver authorizes
    /// and resolves each one as it would its own arguments.
    #[serde(default)]
    pub paths: Vec<String>,
    /// When the request was written, in milliseconds since the Unix epoch.
    pub sent_at_ms: i64,
    /// The 1-based line to show for a path, keyed by the path as it is spelled
    /// in `paths`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub lines: BTreeMap<String, u32>,
    /// The token of a `writ --wait` that is waiting for every tab these paths
    /// open to close. Only a [`is_wait_token`] token is acted on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wait: Option<String>,
    /// `writ://` links, as they were handed over (ADR-052). The receiver reads
    /// each through [`crate::writ_link::parse`], as it reads one the operating
    /// system hands it.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    /// `writ mcp` asking which file is open and what is selected in it, for a
    /// program allowed to see it (ADR-053). A request carrying one opens
    /// nothing and leaves the window where it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextAsk>,
    /// `writ mcp` holding a program's write until the person applies or
    /// discards it (ADR-053). The proposal waits beside it in
    /// `<data dir>/write-reviews/`. A request carrying one opens nothing; the
    /// app brings the window forward itself when it shows the change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review: Option<ReviewAsk>,
}

/// One held write, waiting for the person's answer in
/// `<data dir>/review-answers/<token>.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewAsk {
    /// Names the proposal and the answer. Only an [`is_wait_token`] token is
    /// acted on.
    pub token: String,
    /// The program whose write it is, by the name it sent, which the app
    /// checks against the settings before it shows anything.
    pub program: String,
}

/// One `get_context` call, waiting for the app's answer in
/// `<data dir>/context-answers/<token>.json`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextAsk {
    /// Names the answer file. Only an [`is_wait_token`] token is answered.
    pub token: String,
    /// The program asking, by the name it sent, which the app checks against
    /// the grants in the settings before it answers.
    pub program: String,
}

/// How long a context ask stays worth answering, in milliseconds: the server
/// waits five seconds for the answer, so an older ask has nobody waiting.
pub const CONTEXT_ASK_FRESH_MS: i64 = 10_000;

impl OpenRequest {
    /// Whether the request is still worth acting on at `now_ms`.
    ///
    /// A request stamped in the future (a clock moved back) is acted on: the
    /// cost of opening a file nobody asked for again is a tab, while dropping
    /// one loses what the person just asked for.
    pub fn is_fresh(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.sent_at_ms) <= OPEN_REQUEST_FRESH_MS
    }

    /// The context ask, when the request carries one that is still worth
    /// answering at `now_ms` and whose token may name a file.
    pub fn context_ask(&self, now_ms: i64) -> Option<&ContextAsk> {
        self.context.as_ref().filter(|ask| {
            is_wait_token(&ask.token)
                && now_ms.saturating_sub(self.sent_at_ms) <= CONTEXT_ASK_FRESH_MS
        })
    }

    /// The held write, when the request carries one still waited on at
    /// `now_ms` and whose token may name a file.
    pub fn review_ask(&self, now_ms: i64) -> Option<&ReviewAsk> {
        let waited_ms =
            i64::try_from(crate::write_review::REVIEW_WAIT.as_millis()).unwrap_or(i64::MAX);
        self.review.as_ref().filter(|ask| {
            is_wait_token(&ask.token) && now_ms.saturating_sub(self.sent_at_ms) <= waited_ms
        })
    }

    /// Whether the request asks the window for anything: files, links, or,
    /// with neither and no ask of a program's, to come forward.
    pub fn shows_the_window(&self) -> bool {
        !self.paths.is_empty()
            || !self.links.is_empty()
            || (self.context.is_none() && self.review.is_none())
    }

    /// The wait token, when the request carries one that may name a file.
    pub fn wait_token(&self) -> Option<&str> {
        self.wait.as_deref().filter(|token| is_wait_token(token))
    }
}

/// Whether `token` may name the marker file a waiting `writ` watches.
///
/// ASCII letters, digits and `-` only, at most [`MAX_WAIT_TOKEN_LEN`] of them:
/// the receiver removes `<data dir>/waits/<token>`, and a token holding a
/// separator or `..` would name some other file.
pub fn is_wait_token(token: &str) -> bool {
    !token.is_empty()
        && token.len() <= MAX_WAIT_TOKEN_LEN
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
}

/// A path argument with the line it asks to open at: `Plan.md:12` or
/// `Plan.md:12:4` (the column is read and dropped).
///
/// Only a trailing `:<digits>` or `:<digits>:<digits>` is split, and only when
/// `exists` says the whole argument is not a file itself, so a file whose name
/// ends in `:12` still opens as written. A line of 0 is read as 1.
pub fn split_line_target(arg: &str, exists: impl Fn(&str) -> bool) -> (String, Option<u32>) {
    if exists(arg) {
        return (arg.to_string(), None);
    }
    let Some((head, last)) = arg.rsplit_once(':') else {
        return (arg.to_string(), None);
    };
    let Some(last) = parse_position(last) else {
        return (arg.to_string(), None);
    };
    let (path, line) = match head.rsplit_once(':') {
        Some((path, line)) => match parse_position(line) {
            Some(line) if !path.is_empty() => (path, line),
            _ => (head, last),
        },
        None => (head, last),
    };
    if path.is_empty() || is_drive_prefix(path) {
        return (arg.to_string(), None);
    }
    (path.to_string(), Some(line.max(1)))
}

/// A run of ASCII digits read as a position, `None` for anything else.
fn parse_position(text: &str) -> Option<u32> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// `C`, the drive letter a Windows path such as `C:12` would leave behind.
fn is_drive_prefix(path: &str) -> bool {
    path.len() == 1 && path.bytes().all(|byte| byte.is_ascii_alphabetic())
}

/// The `writ --wait` commands the running instance owes an answer: each
/// token with the files whose tabs have yet to close.
///
/// Paths are compared as the caller spells them, so the caller resolves both
/// sides the same way before asking.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct WaitSet {
    waiting: BTreeMap<String, std::collections::BTreeSet<String>>,
}

impl WaitSet {
    /// Records that `token` waits until every one of `paths` is closed.
    ///
    /// A token with no paths is owed nothing and is answered at once: the
    /// return is whether the caller should release it now.
    pub fn register(&mut self, token: &str, paths: impl IntoIterator<Item = String>) -> bool {
        let paths: std::collections::BTreeSet<String> = paths.into_iter().collect();
        if paths.is_empty() {
            self.waiting.remove(token);
            return true;
        }
        self.waiting
            .entry(token.to_string())
            .or_default()
            .extend(paths);
        false
    }

    /// Marks `closed` as closed and returns every token with nothing left to
    /// wait for, in token order.
    pub fn closed<'a>(&mut self, closed: impl IntoIterator<Item = &'a str>) -> Vec<String> {
        let closed: Vec<&str> = closed.into_iter().collect();
        if closed.is_empty() || self.waiting.is_empty() {
            return Vec::new();
        }
        let mut done = Vec::new();
        self.waiting.retain(|token, paths| {
            for path in &closed {
                paths.remove(*path);
            }
            if paths.is_empty() {
                done.push(token.clone());
                false
            } else {
                true
            }
        });
        done
    }

    /// Whether any command is still waiting.
    pub fn is_empty(&self) -> bool {
        self.waiting.is_empty()
    }
}
