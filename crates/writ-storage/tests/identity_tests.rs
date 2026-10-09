//! What the filesystem calls a file, read the way the app and `writ mcp` read
//! it.

use writ_core::notes::identity::{classify_delete, DeleteVerdict, IdentityProbe};
use writ_storage::identity::{birth_nanos, read_identity, PlatformIdentity};

#[test]
fn the_platform_probe_answers_what_a_read_answers() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("note.md");
    std::fs::write(&path, "body").expect("write");

    assert_eq!(PlatformIdentity.identity_of(&path), read_identity(&path));
    assert!(PlatformIdentity.identity_of(&path).is_some());
}

#[test]
fn the_platform_probe_has_no_answer_for_a_path_holding_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");

    assert!(PlatformIdentity
        .identity_of(&dir.path().join("never-written.md"))
        .is_none());
}

#[test]
fn the_platform_probe_follows_a_file_across_a_rename() {
    let dir = tempfile::tempdir().expect("tempdir");
    let from = dir.path().join("before.md");
    let to = dir.path().join("after.md");
    std::fs::write(&from, "body").expect("write");
    let before = PlatformIdentity.identity_of(&from).expect("identity");
    std::fs::rename(&from, &to).expect("rename");
    let after = PlatformIdentity.identity_of(&to).expect("identity");

    assert_eq!(
        classify_delete(&before, &[(to.clone(), after)]),
        DeleteVerdict::Moved(to)
    );
}

#[test]
fn a_birth_time_is_the_files_own_and_survives_a_rename() {
    // `None` on both sides where the filesystem keeps no birth time, which is
    // still the same answer for the same file.
    let dir = tempfile::tempdir().expect("tempdir");
    let from = dir.path().join("before.md");
    let to = dir.path().join("after.md");
    std::fs::write(&from, "body").expect("write");
    let before = birth_nanos(&std::fs::metadata(&from).expect("metadata"));
    std::fs::rename(&from, &to).expect("rename");

    assert_eq!(
        birth_nanos(&std::fs::metadata(&to).expect("metadata")),
        before
    );
}
