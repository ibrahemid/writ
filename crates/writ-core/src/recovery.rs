use serde::{Deserialize, Serialize};

/// Maximum number of session snapshots retained per database.
///
/// Older snapshots are pruned on each write so the table stays bounded.
/// Five snapshots gives roughly 5 × (autosave interval) of history at
/// minimal storage cost.
pub const MAX_SNAPSHOTS: usize = 5;

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
pub fn resolve_recovery(
    snapshot_created_at: &str,
    buffer_updated_at: &str,
) -> RecoveryResolution {
    if snapshot_created_at > buffer_updated_at {
        RecoveryResolution::Restore
    } else {
        RecoveryResolution::Ignore
    }
}
