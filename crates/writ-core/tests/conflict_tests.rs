use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::conflict::{resolve_conflict, ConflictContext, ConflictResolution};

#[test]
fn modified_without_local_changes_accepts_external() {
    let resolution = resolve_conflict(ConflictContext {
        change: ExternalChange::Modified,
        has_local_changes: false,
    });
    assert_eq!(resolution, ConflictResolution::AcceptExternal);
}

#[test]
fn modified_with_local_changes_prompts_user() {
    let resolution = resolve_conflict(ConflictContext {
        change: ExternalChange::Modified,
        has_local_changes: true,
    });
    assert_eq!(resolution, ConflictResolution::PromptUser);
}

#[test]
fn deleted_keeps_local() {
    let resolution = resolve_conflict(ConflictContext {
        change: ExternalChange::Deleted,
        has_local_changes: false,
    });
    assert_eq!(resolution, ConflictResolution::KeepLocal);
}

#[test]
fn deleted_with_local_changes_keeps_local() {
    let resolution = resolve_conflict(ConflictContext {
        change: ExternalChange::Deleted,
        has_local_changes: true,
    });
    assert_eq!(resolution, ConflictResolution::KeepLocal);
}
