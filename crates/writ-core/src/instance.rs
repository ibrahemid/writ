//! One Writ per data folder (ADR-033, amended 2026-10-14).
//!
//! The process holding `<data dir>/instance.lock` is the only one that opens
//! the database and writes notes. A second launch on the same folder hands the
//! paths it was given to that process as an [`OpenRequest`] and exits.

use serde::{Deserialize, Serialize};

/// How long a request stays worth acting on, in milliseconds.
///
/// A request the running instance never read (it was quitting as the second
/// launch wrote it) is found by the next launch's first look. Opening it then
/// is right a moment later and wrong a day later.
pub const OPEN_REQUEST_FRESH_MS: i64 = 60_000;

/// What a second launch asks the running instance to do: open `paths`, or
/// with none, come forward.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// The paths as the second launch was given them; the receiver authorizes
    /// and resolves each one as it would its own arguments.
    #[serde(default)]
    pub paths: Vec<String>,
    /// When the request was written, in milliseconds since the Unix epoch.
    pub sent_at_ms: i64,
}

impl OpenRequest {
    /// Whether the request is still worth acting on at `now_ms`.
    ///
    /// A request stamped in the future (a clock moved back) is acted on: the
    /// cost of opening a file nobody asked for again is a tab, while dropping
    /// one loses what the person just asked for.
    pub fn is_fresh(&self, now_ms: i64) -> bool {
        now_ms.saturating_sub(self.sent_at_ms) <= OPEN_REQUEST_FRESH_MS
    }
}
