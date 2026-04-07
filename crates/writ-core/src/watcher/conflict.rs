use crate::watcher::change_event::ExternalChange;

#[derive(Debug, Clone, PartialEq)]
pub enum ConflictResolution {
    AcceptExternal,
    KeepLocal,
    PromptUser,
}

pub struct ConflictContext {
    pub change: ExternalChange,
    pub has_local_changes: bool,
}

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
