use crate::watcher::change_event::ExternalChange;

/// Outcome of applying conflict policy to an external change.
#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    /// Discard local edits and load the external version.
    AcceptExternal,
    /// Ignore the external change and keep the local buffer.
    KeepLocal,
    /// Surface the conflict to the user and wait for a decision.
    PromptUser,
}

/// Inputs required to resolve an external-change conflict.
pub struct ConflictContext {
    /// The change observed on disk.
    pub change: ExternalChange,
    /// Whether the buffer has local edits that have not yet been saved.
    pub has_local_changes: bool,
}

/// Resolves an external change into a [`ConflictResolution`] according to
/// Writ's built-in policy.
///
/// The policy is intentionally conservative: deletions never take effect
/// silently, and modifications are only auto-applied when the local
/// buffer is clean.
pub fn resolve_conflict(ctx: ConflictContext) -> ConflictResolution {
    match ctx.change {
        ExternalChange::Deleted => ConflictResolution::KeepLocal,
        ExternalChange::Modified => {
            if ctx.has_local_changes {
                ConflictResolution::PromptUser
            } else {
                ConflictResolution::AcceptExternal
            }
        }
    }
}
