//! The data-folder lock between two processes.
//!
//! A test binary of its own: a process started while another test in the same
//! binary holds a lock could share that lock's open file until it execs, and
//! make the other test's release look as if it never happened.

use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

use writ_storage::instance::{acquire, InstanceLockError};

/// Set on the child process the two-process test starts: the folder it holds.
const HOLDER_DIR_ENV: &str = "WRIT_INSTANCE_LOCK_HOLDER_DIR";

/// Holds the lock on [`HOLDER_DIR_ENV`] until its stdin closes. Run only as the
/// child of [`a_second_process_on_one_folder_is_refused`].
#[test]
#[ignore = "a helper process, started by a_second_process_on_one_folder_is_refused"]
fn instance_lock_holder_process() {
    let Some(dir) = std::env::var_os(HOLDER_DIR_ENV) else {
        return;
    };
    let _lock = acquire(std::path::Path::new(&dir)).expect("holder claim");
    println!("instance-lock-held");
    let mut line = String::new();
    let _ = std::io::stdin().read_line(&mut line);
}

#[test]
fn a_second_process_on_one_folder_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut child = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            "instance_lock_holder_process",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(HOLDER_DIR_ENV, dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the holder");

    let stdout = child.stdout.take().expect("holder stdout");
    let mut lines = BufReader::new(stdout).lines().map_while(Result::ok);
    let held = lines
        .by_ref()
        .any(|line| line.contains("instance-lock-held"));
    assert!(held, "the holder never claimed the folder");

    let refused = acquire(dir.path());
    drop(child.stdin.take());
    // Read to the end, so the holder's last lines never meet a closed pipe.
    lines.for_each(drop);
    let status = child.wait().expect("holder exits");
    assert!(
        matches!(refused, Err(InstanceLockError::Held)),
        "a second process claimed a folder another process holds"
    );
    assert!(status.success());
    acquire(dir.path()).expect("the folder is free once the holder exits");
}
