//! The answers the app leaves in `<data dir>/context-answers/` for a
//! `get_context` call (ADR-053).
//!
//! `writ mcp` asks with an `OpenRequest` carrying a `ContextAsk` in
//! `open-requests/`, and looks for `<token>.json` here while that one call
//! waits. The app writes the answer whole under a name nothing else holds,
//! readable by its owner only, and the server reads it and removes it. An
//! answer nobody took, because the server stopped waiting, is removed at the
//! app's next start ([`clear_answers`]). Nothing here listens or opens a
//! socket: both sides look at files only while a call is waiting.

use std::io;
use std::path::{Path, PathBuf};

use writ_core::instance::is_wait_token;
use writ_core::open_context::OpenContext;

use crate::atomic::write_new;
use crate::errors::StorageResult;

/// The folder answers land in.
const ANSWERS_DIR: &str = "context-answers";

/// Where answers land inside `writ_dir`.
pub fn answers_dir(writ_dir: &Path) -> PathBuf {
    writ_dir.join(ANSWERS_DIR)
}

/// Where the answer for `token` lands, or `None` for a token that could name
/// another file.
pub fn answer_path(writ_dir: &Path, token: &str) -> Option<PathBuf> {
    is_wait_token(token).then(|| answers_dir(writ_dir).join(format!("{token}.json")))
}

/// Leaves `context` for the `get_context` call holding `token`.
///
/// Written whole through [`write_new`], so a reader never sees half an answer
/// and an answer already there is never replaced; the temp file it is written
/// through is created readable by its owner only, and the answer keeps that.
///
/// # Errors
///
/// [`crate::errors::StorageError`] when the token could name another file, or
/// the folder or the file cannot be written.
pub fn write_answer(writ_dir: &Path, token: &str, context: &OpenContext) -> StorageResult<()> {
    let path = answer_path(writ_dir, token)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "not a context token"))?;
    std::fs::create_dir_all(answers_dir(writ_dir))?;
    write_new(&path, &serde_json::to_vec(context)?)?;
    Ok(())
}

/// Takes the answer for `token`, removing it, or `None` while there is none.
///
/// An answer that does not parse is removed and read as no file open: the
/// app wrote it, and a server that cannot read it has nothing better to say.
pub fn take_answer(writ_dir: &Path, token: &str) -> Option<OpenContext> {
    let path = answer_path(writ_dir, token)?;
    let bytes = std::fs::read(&path).ok()?;
    if let Err(error) = std::fs::remove_file(&path) {
        tracing::warn!(%error, "a context answer could not be removed after reading");
    }
    Some(serde_json::from_slice(&bytes).unwrap_or_else(|_| OpenContext::nothing_open()))
}

/// Removes the answer for `token` if one landed, for a call that stopped
/// waiting.
pub fn withdraw_answer(writ_dir: &Path, token: &str) {
    if let Some(path) = answer_path(writ_dir, token) {
        let _ = std::fs::remove_file(path);
    }
}

/// Removes every answer left from an earlier run.
///
/// Called once the app holds the data folder: the server calls those answers
/// were for gave up waiting long ago.
pub fn clear_answers(writ_dir: &Path) {
    let Ok(entries) = std::fs::read_dir(answers_dir(writ_dir)) else {
        return;
    };
    for entry in entries.flatten() {
        if let Err(error) = std::fs::remove_file(entry.path()) {
            tracing::warn!(%error, "an old context answer could not be removed");
        }
    }
}
