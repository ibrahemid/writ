use std::time::{Duration, SystemTime};

use writ_core::hash::sha256_bytes;
use writ_core::notes::guard::{decide_save, DiskState, SaveDecision};

fn state_of(content: &str) -> DiskState {
    DiskState {
        hash: sha256_bytes(content.as_bytes()),
        size: content.len() as u64,
        mtime: Some(SystemTime::UNIX_EPOCH),
    }
}

#[test]
fn unchanged_disk_proceeds() {
    let last_known = state_of("what Writ last read");
    let on_disk = state_of("what Writ last read");

    assert_eq!(
        decide_save(
            Some(&last_known),
            Some(&on_disk),
            sha256_bytes(b"what the user typed")
        ),
        SaveDecision::Proceed
    );
}

#[test]
fn missing_file_proceeds() {
    let last_known = state_of("what Writ last read");

    assert_eq!(
        decide_save(
            Some(&last_known),
            None,
            sha256_bytes(b"what the user typed")
        ),
        SaveDecision::Proceed
    );
}

#[test]
fn no_last_known_proceeds() {
    let on_disk = state_of("a file Writ has never read");

    assert_eq!(
        decide_save(None, Some(&on_disk), sha256_bytes(b"the first save")),
        SaveDecision::Proceed
    );
}

#[test]
fn disk_changed_but_identical_to_incoming_succeeds_silently() {
    let last_known = state_of("the old text");
    let on_disk = state_of("the same edit, made twice");

    assert_eq!(
        decide_save(
            Some(&last_known),
            Some(&on_disk),
            sha256_bytes(b"the same edit, made twice")
        ),
        SaveDecision::AlreadyIdentical
    );
}

#[test]
fn disk_changed_and_differs_refuses() {
    let last_known = state_of("the old text");
    let on_disk = state_of("what another program wrote");

    assert_eq!(
        decide_save(
            Some(&last_known),
            Some(&on_disk),
            sha256_bytes(b"what the user typed")
        ),
        SaveDecision::Refuse
    );
}

#[test]
fn mtime_change_alone_never_refuses() {
    let last_known = state_of("untouched text");
    let on_disk = DiskState {
        mtime: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(86_400)),
        ..state_of("untouched text")
    };

    assert_eq!(last_known.size, on_disk.size);
    assert_ne!(last_known.mtime, on_disk.mtime);
    assert_eq!(
        decide_save(
            Some(&last_known),
            Some(&on_disk),
            sha256_bytes(b"what the user typed")
        ),
        SaveDecision::Proceed
    );
}
