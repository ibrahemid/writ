//! The data-folder lock and the requests a second launch leaves.

use writ_core::instance::OpenRequest;
use writ_storage::instance::{
    acquire, lock_path, requests_dir, send_request, take_requests, InstanceLockError,
};

fn request(paths: &[&str], sent_at_ms: i64) -> OpenRequest {
    OpenRequest {
        paths: paths.iter().map(|path| path.to_string()).collect(),
        sent_at_ms,
    }
}

#[test]
fn a_second_claim_on_one_folder_is_refused_while_the_first_stands() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = acquire(dir.path()).expect("first claim");
    assert!(matches!(acquire(dir.path()), Err(InstanceLockError::Held)));
    drop(first);
    acquire(dir.path()).expect("the folder is free once the first claim is dropped");
}

#[test]
fn claiming_creates_the_folder_and_the_lock_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let writ_dir = dir.path().join("data");
    let _lock = acquire(&writ_dir).expect("claim");
    assert!(lock_path(&writ_dir).is_file());
}

#[test]
fn two_folders_are_claimed_independently() {
    let a = tempfile::tempdir().expect("tempdir");
    let b = tempfile::tempdir().expect("tempdir");
    let _first = acquire(a.path()).expect("a");
    acquire(b.path()).expect("another folder is not held by the first claim");
}

#[test]
fn requests_come_back_oldest_first_and_are_taken_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    send_request(dir.path(), &request(&["/b.md"], 2_000)).expect("second");
    send_request(dir.path(), &request(&["/a.md"], 1_000)).expect("first");
    send_request(dir.path(), &request(&[], 3_000)).expect("focus only");

    let taken = take_requests(dir.path());
    assert_eq!(
        taken,
        vec![
            request(&["/a.md"], 1_000),
            request(&["/b.md"], 2_000),
            request(&[], 3_000),
        ]
    );
    assert!(take_requests(dir.path()).is_empty());
    let left = std::fs::read_dir(requests_dir(dir.path()))
        .expect("folder")
        .count();
    assert_eq!(left, 0, "every taken request is removed");
}

#[test]
fn a_temp_file_and_a_file_that_does_not_parse_are_never_returned() {
    let dir = tempfile::tempdir().expect("tempdir");
    let folder = requests_dir(dir.path());
    std::fs::create_dir_all(&folder).expect("folder");
    std::fs::write(folder.join(".writ-tmp-half"), b"{\"paths\":[\"/x").expect("temp");
    std::fs::write(folder.join("0-1-0.json"), b"not json").expect("garbage");
    send_request(dir.path(), &request(&["/a.md"], 5)).expect("real");

    assert_eq!(take_requests(dir.path()), vec![request(&["/a.md"], 5)]);
    assert!(!folder.join("0-1-0.json").exists(), "garbage is cleared");
    assert!(
        folder.join(".writ-tmp-half").exists(),
        "a temp file belongs to a writer still writing"
    );
}

#[test]
fn no_folder_means_no_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(take_requests(dir.path()).is_empty());
}
