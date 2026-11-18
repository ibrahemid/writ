//! The data-folder lock and the requests a second launch leaves.

use jotra_core::instance::OpenRequest;
use jotra_storage::instance::{
    acquire, lock_path, requests_dir, send_request, take_requests, InstanceLockError,
};

fn request(paths: &[&str], sent_at_ms: i64) -> OpenRequest {
    OpenRequest {
        paths: paths.iter().map(|path| path.to_string()).collect(),
        sent_at_ms,
        ..OpenRequest::default()
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
    let jotra_dir = dir.path().join("data");
    let _lock = acquire(&jotra_dir).expect("claim");
    assert!(lock_path(&jotra_dir).is_file());
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
    std::fs::write(folder.join(".jotra-tmp-half"), b"{\"paths\":[\"/x").expect("temp");
    std::fs::write(folder.join("0-1-0.json"), b"not json").expect("garbage");
    send_request(dir.path(), &request(&["/a.md"], 5)).expect("real");

    assert_eq!(take_requests(dir.path()), vec![request(&["/a.md"], 5)]);
    assert!(!folder.join("0-1-0.json").exists(), "garbage is cleared");
    assert!(
        folder.join(".jotra-tmp-half").exists(),
        "a temp file belongs to a writer still writing"
    );
}

#[test]
fn no_folder_means_no_requests() {
    let dir = tempfile::tempdir().expect("tempdir");
    assert!(take_requests(dir.path()).is_empty());
}

#[test]
fn a_wait_marker_is_left_and_released_by_its_token() {
    let dir = tempfile::tempdir().expect("tempdir");
    let token = jotra_storage::instance::new_wait_token();
    let marker = jotra_storage::instance::create_wait_marker(dir.path(), &token).expect("marker");
    assert!(marker.is_file());
    assert!(marker.starts_with(jotra_storage::instance::waits_dir(dir.path())));
    jotra_storage::instance::release_wait(dir.path(), &token);
    assert!(!marker.exists());
    jotra_storage::instance::release_wait(dir.path(), &token);
}

#[test]
fn two_tokens_are_never_the_same() {
    let first = jotra_storage::instance::new_wait_token();
    let second = jotra_storage::instance::new_wait_token();
    assert_ne!(first, second);
    assert!(jotra_core::instance::is_wait_token(&first), "{first}");
}

#[test]
fn a_token_that_names_another_file_removes_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let bystander = dir.path().join("config.toml");
    std::fs::write(&bystander, "kept").expect("write");
    std::fs::create_dir_all(jotra_storage::instance::waits_dir(dir.path())).expect("waits");
    jotra_storage::instance::release_wait(dir.path(), "../config.toml");
    assert!(bystander.is_file());
    assert!(jotra_storage::instance::create_wait_marker(dir.path(), "../config.toml").is_err());
}

#[test]
fn clearing_answers_every_wait_left_from_an_earlier_run() {
    let dir = tempfile::tempdir().expect("tempdir");
    let a = jotra_storage::instance::create_wait_marker(dir.path(), "a1").expect("a");
    let b = jotra_storage::instance::create_wait_marker(dir.path(), "b2").expect("b");
    jotra_storage::instance::clear_waits(dir.path());
    assert!(!a.exists() && !b.exists());
}

#[test]
fn a_request_carrying_a_line_and_a_wait_is_taken_whole() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut sent = request(&["/notes/a.md"], 7);
    sent.lines.insert("/notes/a.md".to_string(), 3);
    sent.wait = Some("t1".to_string());
    send_request(dir.path(), &sent).expect("send");
    assert_eq!(take_requests(dir.path()), vec![sent]);
}

#[test]
fn clearing_keeps_the_wait_of_a_request_still_waiting_to_be_taken() {
    let dir = tempfile::tempdir().expect("tempdir");
    let pending = jotra_storage::instance::create_wait_marker(dir.path(), "pending1").expect("p");
    let orphan = jotra_storage::instance::create_wait_marker(dir.path(), "orphan1").expect("o");
    let mut waiting = request(&["/notes/msg.txt"], 7);
    waiting.wait = Some("pending1".to_string());
    send_request(dir.path(), &waiting).expect("send");

    jotra_storage::instance::clear_waits(dir.path());

    assert!(
        pending.exists(),
        "the request that started the app still waits"
    );
    assert!(!orphan.exists(), "a wait no request names is answered");
    assert_eq!(take_requests(dir.path()), vec![waiting]);
}
