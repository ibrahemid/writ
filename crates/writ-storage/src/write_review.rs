//! The files a held write travels in between `writ mcp` and the app
//! (ADR-053).
//!
//! `writ mcp` leaves the proposal in `<data dir>/write-reviews/<token>.json`
//! and an `OpenRequest` carrying a `ReviewAsk` in `open-requests/`. The app
//! reads the proposal, shows it, and leaves the person's answer in
//! `review-answers/<token>.json`, which the server takes. Both are written
//! whole under names nothing else holds and readable by their owner only.
//! Anything a server stopped waiting for is removed at the app's next start
//! ([`clear_reviews`]).

use std::io;
use std::path::{Path, PathBuf};

use writ_core::instance::is_wait_token;
use writ_core::write_review::{ReviewAnswer, WriteProposal};

use crate::atomic::write_new;
use crate::errors::StorageResult;

/// The folder proposals wait in.
const PROPOSALS_DIR: &str = "write-reviews";

/// The folder answers land in.
const ANSWERS_DIR: &str = "review-answers";

/// Where proposals wait inside `writ_dir`.
pub fn proposals_dir(writ_dir: &Path) -> PathBuf {
    writ_dir.join(PROPOSALS_DIR)
}

/// Where answers land inside `writ_dir`.
pub fn answers_dir(writ_dir: &Path) -> PathBuf {
    writ_dir.join(ANSWERS_DIR)
}

fn named(dir: PathBuf, token: &str) -> Option<PathBuf> {
    is_wait_token(token).then(|| dir.join(format!("{token}.json")))
}

fn not_a_token() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, "not a review token")
}

/// Leaves `proposal` for the app under `token`.
///
/// # Errors
///
/// [`crate::errors::StorageError`] when the token could name another file, or
/// the folder or the file cannot be written.
pub fn write_proposal(writ_dir: &Path, token: &str, proposal: &WriteProposal) -> StorageResult<()> {
    let path = named(proposals_dir(writ_dir), token).ok_or_else(not_a_token)?;
    std::fs::create_dir_all(proposals_dir(writ_dir))?;
    write_new(&path, &serde_json::to_vec(proposal)?)?;
    Ok(())
}

/// The proposal waiting under `token`, or `None` when there is none or it
/// does not parse.
pub fn read_proposal(writ_dir: &Path, token: &str) -> Option<WriteProposal> {
    let path = named(proposals_dir(writ_dir), token)?;
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Leaves the person's answer for the write holding `token`.
///
/// # Errors
///
/// [`crate::errors::StorageError`] when the token could name another file, or
/// the folder or the file cannot be written.
pub fn write_answer(writ_dir: &Path, token: &str, answer: ReviewAnswer) -> StorageResult<()> {
    let path = named(answers_dir(writ_dir), token).ok_or_else(not_a_token)?;
    std::fs::create_dir_all(answers_dir(writ_dir))?;
    write_new(&path, &serde_json::to_vec(&answer)?)?;
    Ok(())
}

/// Takes the answer for `token`, removing it and the proposal, or `None`
/// while there is none.
///
/// An answer that does not parse is read as a discard: only a clear Apply
/// lets a held write through.
pub fn take_answer(writ_dir: &Path, token: &str) -> Option<ReviewAnswer> {
    let path = named(answers_dir(writ_dir), token)?;
    let bytes = std::fs::read(&path).ok()?;
    withdraw(writ_dir, token);
    Some(serde_json::from_slice(&bytes).unwrap_or(ReviewAnswer::Discard))
}

/// Removes the proposal and any answer for `token`, for a write that stopped
/// waiting or was answered.
pub fn withdraw(writ_dir: &Path, token: &str) {
    for dir in [proposals_dir(writ_dir), answers_dir(writ_dir)] {
        if let Some(path) = named(dir, token) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Removes every proposal and answer left from an earlier run.
///
/// Called once the app holds the data folder: the writes they were for gave
/// up waiting when the app before this one ended.
pub fn clear_reviews(writ_dir: &Path) {
    for dir in [proposals_dir(writ_dir), answers_dir(writ_dir)] {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            if let Err(error) = std::fs::remove_file(entry.path()) {
                tracing::warn!(%error, "an old held write could not be removed");
            }
        }
    }
}
