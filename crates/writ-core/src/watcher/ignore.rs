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

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Default lifetime of an ignore stamp before it is considered stale.
///
/// Picked to comfortably exceed the watcher debounce window (500ms) plus
/// any plausible scheduling slack, while staying short enough that a
/// stamp whose write actually failed cannot suppress a future unrelated
/// event for the same file.
pub const DEFAULT_IGNORE_TTL: Duration = Duration::from_secs(5);

/// SHA-256 digest of buffer file content.
pub type ContentHash = [u8; 32];

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

/// Map of pending internal-write fingerprints, keyed by buffer filename.
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

    /// Records the fingerprint of `content` for `filename` at `now`.
    ///
    /// Uses [`DEFAULT_IGNORE_TTL`]; pre-evicts any stamps older than that
    /// to bound the map size.
    pub fn record(&mut self, filename: String, content: &[u8], now: Instant) {
        self.record_with_ttl(filename, content, now, DEFAULT_IGNORE_TTL);
    }

    /// Records the fingerprint of `content` for `filename` at `now`, with
    /// an explicit `ttl` controlling the opportunistic eviction sweep.
    pub fn record_with_ttl(
        &mut self,
        filename: String,
        content: &[u8],
        now: Instant,
        ttl: Duration,
    ) {
        self.evict_expired(now, ttl);
        let hash = hash_bytes(content);
        self.inner.insert(filename, IgnoreStamp { hash, at: now });
    }

    /// Decides whether a delivered event for `filename` should be emitted
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
        filename: &str,
        current_disk_content: Option<&[u8]>,
        now: Instant,
        ttl: Duration,
    ) -> SuppressDecision {
        let Some(stamp) = self.inner.get(filename).copied() else {
            return SuppressDecision::Emit;
        };

        if now.saturating_duration_since(stamp.at) > ttl {
            self.inner.remove(filename);
            return SuppressDecision::Emit;
        }

        let Some(bytes) = current_disk_content else {
            self.inner.remove(filename);
            return SuppressDecision::Emit;
        };

        if hash_bytes(bytes) == stamp.hash {
            SuppressDecision::Suppress
        } else {
            self.inner.remove(filename);
            SuppressDecision::Emit
        }
    }

    /// Removes any stamp for `filename`. Used by close/delete commands.
    pub fn remove(&mut self, filename: &str) {
        self.inner.remove(filename);
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

    /// Returns `true` if a stamp is currently recorded for `filename`.
    pub fn contains(&self, filename: &str) -> bool {
        self.inner.contains_key(filename)
    }
}

/// Computes the SHA-256 fingerprint of `content`.
pub fn hash_bytes(content: &[u8]) -> ContentHash {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let result = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&result);
    out
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
