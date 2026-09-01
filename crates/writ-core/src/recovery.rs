use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Maximum number of session snapshots retained per database.
///
/// Older snapshots are pruned on each write so the table stays bounded.
/// Five snapshots gives roughly 5 × (autosave interval) of history at
/// minimal storage cost.
pub const MAX_SNAPSHOTS: usize = 5;

/// How long the shutdown path waits for the frontend to confirm it flushed
/// before exiting anyway. A quit that hangs on an unresponsive webview is a
/// worse failure than a quit that loses the last debounce window.
pub const QUIT_FLUSH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(2000);

/// Whether the shutdown path should stop waiting and exit.
pub fn should_force_exit(waited: std::time::Duration, flush_confirmed: bool) -> bool {
    flush_confirmed || waited >= QUIT_FLUSH_TIMEOUT
}

/// A single buffer whose content was restored from a crash snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredBuffer {
    /// Buffer id.
    pub id: String,
    /// Content recovered from the snapshot.
    pub content: String,
}

/// Outcome of comparing a snapshot entry against the persisted buffer state.
#[derive(Debug, Clone, PartialEq)]
pub enum RecoveryResolution {
    /// The snapshot is newer than the stored buffer; use snapshot content.
    Restore,
    /// The stored buffer is at least as recent; discard snapshot entry.
    Ignore,
    /// No snapshot entry exists for this buffer; nothing to do.
    NoSnapshot,
}

/// Compares timestamps to decide whether a snapshot entry should override
/// the stored buffer content.
///
/// `snapshot_created_at` and `buffer_updated_at` are ISO 8601 strings as
/// produced by SQLite's `datetime('now')`.
pub fn resolve_recovery(snapshot_created_at: &str, buffer_updated_at: &str) -> RecoveryResolution {
    if snapshot_created_at > buffer_updated_at {
        RecoveryResolution::Restore
    } else {
        RecoveryResolution::Ignore
    }
}

/// Stable fingerprint of a set of buffer contents.
///
/// Two collections with the same buffer ids and the same content produce the
/// same fingerprint regardless of iteration order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SnapshotFingerprint([u8; 32]);

impl SnapshotFingerprint {
    /// Returns the raw 32-byte digest.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Computes the fingerprint of the buffer contents destined for a snapshot.
///
/// Ids are sorted so a `HashMap`'s iteration order cannot change the result,
/// and each id and content is length-prefixed so no pair of buffers can be
/// concatenated into the same byte stream as a different pair.
pub fn fingerprint_buffers(buffer_contents: &HashMap<String, String>) -> SnapshotFingerprint {
    let mut ids: Vec<&String> = buffer_contents.keys().collect();
    ids.sort();

    let mut hasher = Sha256::new();
    for id in ids {
        let content = &buffer_contents[id];
        hasher.update((id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
        hasher.update((content.len() as u64).to_le_bytes());
        hasher.update(content.as_bytes());
    }
    let digest = hasher.finalize();
    let mut out = [0u8; 32];
    out.copy_from_slice(&digest);
    SnapshotFingerprint(out)
}

/// Decides whether a heartbeat snapshot is worth writing.
///
/// A snapshot is written only when the content differs from the last one
/// persisted. Writing an identical snapshot every heartbeat costs an insert
/// and a delete per interval and leaves the freed pages behind, which is what
/// grew the database without bound.
pub fn should_snapshot(
    previous: Option<SnapshotFingerprint>,
    current: SnapshotFingerprint,
) -> bool {
    previous != Some(current)
}

/// The snapshot a shutdown records, and whether that shutdown counts as clean.
///
/// `on_disk` is what each open note's file holds; `unsaved` is text a save
/// could not write, keyed the same way. The unsaved text wins every collision,
/// because the file it never reached is exactly the copy that is behind.
///
/// A shutdown that left text nowhere but here is not a clean one. Only a
/// snapshot marked unclean is read back on the next launch
/// ([`resolve_recovery`] is reached through it), so marking this one clean
/// would discard the text the overlay exists to keep.
pub fn shutdown_snapshot(
    mut on_disk: HashMap<String, String>,
    unsaved: HashMap<String, String>,
) -> (HashMap<String, String>, bool) {
    let is_clean = unsaved.is_empty();
    on_disk.extend(unsaved);
    (on_disk, is_clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map(entries: &[(&str, &str)]) -> HashMap<String, String> {
        entries
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn fingerprint_is_stable_across_insertion_order() {
        let a = map(&[("buf-1", "alpha"), ("buf-2", "beta")]);
        let mut b = HashMap::new();
        b.insert("buf-2".to_string(), "beta".to_string());
        b.insert("buf-1".to_string(), "alpha".to_string());

        assert_eq!(fingerprint_buffers(&a), fingerprint_buffers(&b));
    }

    #[test]
    fn fingerprint_changes_when_content_changes() {
        let before = map(&[("buf-1", "alpha")]);
        let after = map(&[("buf-1", "alpha!")]);

        assert_ne!(fingerprint_buffers(&before), fingerprint_buffers(&after));
    }

    #[test]
    fn fingerprint_changes_when_a_buffer_is_added_or_removed() {
        let one = map(&[("buf-1", "alpha")]);
        let two = map(&[("buf-1", "alpha"), ("buf-2", "")]);

        assert_ne!(fingerprint_buffers(&one), fingerprint_buffers(&two));
    }

    #[test]
    fn fingerprint_separates_ids_from_content() {
        let split = map(&[("ab", "cd")]);
        let shifted = map(&[("a", "bcd")]);

        assert_ne!(fingerprint_buffers(&split), fingerprint_buffers(&shifted));
    }

    #[test]
    fn empty_collections_fingerprint_equal() {
        assert_eq!(
            fingerprint_buffers(&HashMap::new()),
            fingerprint_buffers(&HashMap::new())
        );
    }

    #[test]
    fn should_snapshot_is_true_without_a_previous_fingerprint() {
        let current = fingerprint_buffers(&map(&[("buf-1", "alpha")]));
        assert!(should_snapshot(None, current));
    }

    #[test]
    fn should_snapshot_is_false_when_nothing_changed() {
        let current = fingerprint_buffers(&map(&[("buf-1", "alpha")]));
        assert!(!should_snapshot(Some(current), current));
    }

    #[test]
    fn should_snapshot_is_true_after_an_edit() {
        let previous = fingerprint_buffers(&map(&[("buf-1", "alpha")]));
        let current = fingerprint_buffers(&map(&[("buf-1", "alpha beta")]));
        assert!(should_snapshot(Some(previous), current));
    }

    #[test]
    fn a_shutdown_with_nothing_unsaved_is_clean_and_records_the_files() {
        let (contents, is_clean) = shutdown_snapshot(map(&[("a", "on disk")]), HashMap::new());
        assert!(is_clean);
        assert_eq!(contents, map(&[("a", "on disk")]));
    }

    #[test]
    fn text_a_save_could_not_write_replaces_what_the_file_holds() {
        let (contents, is_clean) = shutdown_snapshot(
            map(&[("a", "on disk"), ("b", "kept")]),
            map(&[("a", "typed")]),
        );
        assert!(!is_clean);
        assert_eq!(contents, map(&[("a", "typed"), ("b", "kept")]));
    }

    #[test]
    fn a_note_with_no_readable_file_still_reaches_the_snapshot() {
        let (contents, is_clean) = shutdown_snapshot(HashMap::new(), map(&[("a", "typed")]));
        assert!(!is_clean);
        assert_eq!(contents, map(&[("a", "typed")]));
    }
}
