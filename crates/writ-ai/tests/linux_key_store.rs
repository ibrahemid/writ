//! A key stored through the Secret Service outlives the process that stored it.
//!
//! Runs on Linux when `WRIT_SECRET_SERVICE_TEST=1`, which CI's Linux job sets
//! inside a D-Bus session with an unlocked GNOME Keyring. Anywhere else each
//! test returns at once: the ordinary test step has no session bus, and a
//! developer's machine is not written to. With the variable set, a Secret
//! Service that does not answer fails the test rather than skipping it.
#![cfg(target_os = "linux")]

use std::process::Command;

use writ_ai::keys::platform_key_store;
use writ_ai::AiState;

/// An account no provider row uses, so the run touches nobody's key.
const ACCOUNT: &str = "writ-ci-secret-service";
const KEY: &str = "sk-written-by-one-process";

/// Set on the child run that plays the relaunched app.
const RELAUNCH: &str = "WRIT_SECRET_SERVICE_RELAUNCH";

fn enabled() -> bool {
    std::env::var("WRIT_SECRET_SERVICE_TEST").as_deref() == Ok("1")
}

#[test]
fn a_key_saved_in_one_process_is_read_by_the_next() {
    if !enabled() || std::env::var_os(RELAUNCH).is_some() {
        return;
    }

    let saved = AiState::new()
        .set_key(ACCOUNT, KEY)
        .expect("a key that is not empty is taken");
    assert!(
        saved.is_set && !saved.memory_only,
        "the Secret Service did not take the key, so the key row would say it is not saved"
    );

    let relaunch = Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "the_relaunched_process_reads_the_key",
            "--nocapture",
        ])
        .env(RELAUNCH, "1")
        .output()
        .expect("the relaunch ran");

    let cleared = platform_key_store().delete(ACCOUNT);
    assert!(
        relaunch.status.success(),
        "the relaunched process did not read the key:\n{}{}",
        String::from_utf8_lossy(&relaunch.stdout),
        String::from_utf8_lossy(&relaunch.stderr)
    );
    assert!(
        String::from_utf8_lossy(&relaunch.stdout).contains("1 passed"),
        "the relaunched process ran no test:\n{}",
        String::from_utf8_lossy(&relaunch.stdout)
    );
    cleared.expect("the test key was removed");
}

#[test]
fn the_relaunched_process_reads_the_key() {
    if !enabled() || std::env::var_os(RELAUNCH).is_none() {
        return;
    }
    let relaunched = AiState::new();
    let state = relaunched.key_state(ACCOUNT);
    assert!(state.is_set && !state.memory_only, "{state:?}");
    assert_eq!(relaunched.key(ACCOUNT).as_deref(), Some(KEY));
}
