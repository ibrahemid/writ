//! Content-fingerprinted suppression of internal writes.
//!
//! The filesystem watcher used by `writ-tauri` debounces events over a
//! short window. When Writ writes to a buffer file at the same time as an
//! external process, the debouncer can collapse both events into a single
//! delivered event. A naive "set of pending internal filenames" model
//! cannot tell those two cases apart and silently drops the external edit.
//!
//! [`IgnoreStamps`] solves this by recording the SHA-256 of the bytes
//! Writ *intends to write* before issuing the write, alongside an
//! [`std::time::Instant`]. When an event arrives, the watcher reads the
//! file's current bytes, hashes them, and only suppresses the event when
//! the observed hash matches the recorded stamp and the stamp is younger
//! than a TTL. Anything else (mismatch, stale stamp, missing file, no
//! stamp at all) is treated as a real external change and emitted.
//!
//! All time inputs are passed in explicitly so callers can test the
//! decision deterministically without a real clock.

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

/// Default lifetime of an ignore stamp before it is considered stale.
///
/// Picked to comfortably exceed the watcher debounce window (500ms) plus
/// any plausible scheduling slack, while staying short enough that a
/// stamp whose write actually failed cannot suppress a future unrelated
/// event for the same file.
pub const DEFAULT_IGNORE_TTL: Duration = Duration::from_secs(5);

/// SHA-256 digest of buffer file content.
pub type ContentHash = crate::hash::Sha256Digest;

/// A recorded "Writ is about to write these bytes" assertion for a single
/// file, used to recognize the resulting filesystem event as internal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoreStamp {
    /// SHA-256 of the bytes Writ intended to write.
    pub hash: ContentHash,
    /// Wall-clock-independent timestamp captured at insert time.
    pub at: Instant,
}

/// Outcome of applying the ignore policy to a single delivered event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SuppressDecision {
    /// The event matches a fresh internal write; do not surface it.
    Suppress,
    /// Treat the event as a real external change.
    Emit,
}

/// Map of pending internal-write fingerprints, keyed by a namespaced
/// canonical path ([`source_key`], [`config_key`]).
///
/// Insertion is performed by IPC commands immediately before issuing
/// their write. Lookup is performed by the watcher when an event for a
/// buffer file is delivered. A stamp is retained as long as the file's
/// on-disk bytes still match it within the TTL window, so every event
/// produced by a single internal write is suppressed; the stamp is
/// evicted only when an event proves it stale (TTL exceeded, file gone,
/// or bytes diverged).
#[derive(Debug, Default)]
pub struct IgnoreStamps {
    inner: HashMap<String, IgnoreStamp>,
}

impl IgnoreStamps {
    /// Creates an empty stamp map.
    pub fn new() -> Self {
        Self {
            inner: HashMap::new(),
        }
    }

    /// Records the fingerprint of `content` for `key` at `now`.
    ///
    /// Uses [`DEFAULT_IGNORE_TTL`]; pre-evicts any stamps older than that
    /// to bound the map size.
    pub fn record(&mut self, key: String, content: &[u8], now: Instant) {
        self.record_with_ttl(key, content, now, DEFAULT_IGNORE_TTL);
    }

    /// Records the fingerprint of `content` for `key` at `now`, with
    /// an explicit `ttl` controlling the opportunistic eviction sweep.
    pub fn record_with_ttl(&mut self, key: String, content: &[u8], now: Instant, ttl: Duration) {
        self.evict_expired(now, ttl);
        let hash = hash_bytes(content);
        self.inner.insert(key, IgnoreStamp { hash, at: now });
    }

    /// Decides whether a delivered event for `key` should be emitted
    /// or suppressed.
    ///
    /// `current_disk_content` is the file's current on-disk bytes as
    /// observed by the watcher, or `None` if the file no longer exists
    /// (deleted).
    ///
    /// A matching stamp is KEPT when the decision is [`SuppressDecision::Suppress`]
    /// (observed bytes equal the recorded fingerprint and the stamp is
    /// within `ttl`), so every event a single atomic write fans out into
    /// is suppressed. The stamp is removed only when the decision is
    /// [`SuppressDecision::Emit`]: the stamp is stale (older than `ttl`),
    /// the file is gone (`None`), or the observed bytes differ from the
    /// fingerprint (a genuine external edit).
    pub fn decide(
        &mut self,
        key: &str,
        current_disk_content: Option<&[u8]>,
        now: Instant,
        ttl: Duration,
    ) -> SuppressDecision {
        let Some(stamp) = self.inner.get(key).copied() else {
            return SuppressDecision::Emit;
        };

        if now.saturating_duration_since(stamp.at) > ttl {
            self.inner.remove(key);
            return SuppressDecision::Emit;
        }

        let Some(bytes) = current_disk_content else {
            self.inner.remove(key);
            return SuppressDecision::Emit;
        };

        if hash_bytes(bytes) == stamp.hash {
            SuppressDecision::Suppress
        } else {
            self.inner.remove(key);
            SuppressDecision::Emit
        }
    }

    /// Removes any stamp for `key`. Used by close/delete commands.
    pub fn remove(&mut self, key: &str) {
        self.inner.remove(key);
    }

    /// Drops every stamp older than `ttl` relative to `now`.
    pub fn evict_expired(&mut self, now: Instant, ttl: Duration) {
        self.inner
            .retain(|_, stamp| now.saturating_duration_since(stamp.at) <= ttl);
    }

    /// Returns the current number of recorded stamps.
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns `true` if no stamps are currently recorded.
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Returns `true` if a stamp is currently recorded for `key`.
    pub fn contains(&self, key: &str) -> bool {
        self.inner.contains_key(key)
    }
}

/// Namespace for the file a note's text lives in.
const SOURCE_NAMESPACE: &str = "source";

/// Namespace for the config file.
const CONFIG_NAMESPACE: &str = "config";

/// The stamp key for the file a note lives in.
///
/// `canonical` must be a path that came through [`std::fs::canonicalize`], or,
/// for a file that does not exist yet, one whose existing ancestors did.
/// Canonicalisation resolves symlinks and rewrites `/var` to `/private/var`,
/// so a key built from an unresolved path can never match the path the watcher
/// delivers for the same file, and every save would arrive as somebody else's
/// edit.
///
/// The key carries a namespace because a bare filename is a global one: two
/// notes named `index.md` in different folders share it, and so does the
/// config file, so a save of one suppresses a real change to the other for as
/// long as the stamp lives (ADR-028 section 6).
///
/// Lowercased on macOS and Windows, whose filesystems are case-preserving but
/// case-insensitive, so a case-only rename APFS and NTFS perform in place does
/// not orphan the stamp. Byte-exact on Linux, where two spellings are two
/// files.
pub fn source_key(canonical: &Path) -> String {
    namespaced_key(SOURCE_NAMESPACE, canonical)
}

/// The stamp key for the config file, under its own namespace so a note
/// named `config.toml` cannot suppress a real config reload.
///
/// Same canonicalisation contract as [`source_key`].
pub fn config_key(canonical: &Path) -> String {
    namespaced_key(CONFIG_NAMESPACE, canonical)
}

fn namespaced_key(namespace: &str, canonical: &Path) -> String {
    let path = canonical.to_string_lossy();
    if cfg!(any(target_os = "macos", target_os = "windows")) {
        format!("{namespace}:{}", path.to_lowercase())
    } else {
        format!("{namespace}:{path}")
    }
}

/// Computes the SHA-256 fingerprint of `content`.
///
/// One digest is shared with the write guard and the notes migration, so a
/// stamp and a verification can never disagree about what a file holds.
pub fn hash_bytes(content: &[u8]) -> ContentHash {
    crate::hash::sha256_bytes(content)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TTL: Duration = Duration::from_secs(5);

    #[test]
    fn keeps_stamp_so_repeated_events_from_one_write_are_all_suppressed() {
        let mut stamps = IgnoreStamps::new();
        let t0 = Instant::now();
        let bytes = b"one atomic write";

        stamps.record("draft.txt".to_string(), bytes, t0);

        assert_eq!(
            stamps.decide("draft.txt", Some(bytes), t0, TTL),
            SuppressDecision::Suppress
        );
        assert_eq!(
            stamps.decide("draft.txt", Some(bytes), t0, TTL),
            SuppressDecision::Suppress
        );
        assert!(stamps.contains("draft.txt"));
    }

    #[test]
    fn external_edit_after_internal_write_emits_and_clears() {
        let mut stamps = IgnoreStamps::new();
        let t0 = Instant::now();
        let bytes_a = b"writ wrote this";
        let bytes_b = b"someone else wrote this";

        stamps.record("draft.txt".to_string(), bytes_a, t0);

        assert_eq!(
            stamps.decide("draft.txt", Some(bytes_b), t0, TTL),
            SuppressDecision::Emit
        );
        assert!(!stamps.contains("draft.txt"));
    }

    #[test]
    fn stale_matching_stamp_emits_and_clears() {
        let mut stamps = IgnoreStamps::new();
        let t0 = Instant::now();
        let bytes = b"matching but stale";

        stamps.record("draft.txt".to_string(), bytes, t0);

        let later = t0 + TTL + Duration::from_millis(1);
        assert_eq!(
            stamps.decide("draft.txt", Some(bytes), later, TTL),
            SuppressDecision::Emit
        );
        assert!(!stamps.contains("draft.txt"));
    }
}
