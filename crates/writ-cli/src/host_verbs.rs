//! The verbs that run on the note host, the surface `writ mcp` serves from
//! (ADR-032): `search`, `cat`, `ls`, `append`, `today` and `open`.
//!
//! A `<file>` argument is resolved the way every other verb resolves it
//! ([`crate::verbs::note_file`], then the index), and the host is handed the
//! absolute path, so a relative path means the current directory here as it
//! does everywhere else on the command line. The host then refuses a file
//! outside the notes folder, as it does for a connected program.
//!
//! The writes go through the host's guarded writes under the data folder's
//! write lock, keep a version of what they replace in the app's version store
//! when the file is in the app's folder, and are made in the file's own
//! encoding and line ending (ADR-049).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use writ_core::notes::containment::resolve_for_containment;
use writ_core::notes::host::{Capability, HostError, NoteHost, PermissionSet, MAX_NOTE_BYTES};
use writ_core::notes::WriteOrigin;
use writ_storage::identity::PlatformIdentity;
use writ_storage::note_history::NoteHistoryStore;
use writ_storage::note_host::NoteHostImpl;
use writ_storage::note_ops::{find_todays_file, TodaysFileAt};
use writ_storage::write_lock::WriteLock;

use crate::verbs::{
    note_file, note_key, open_index, path_outcome, plain_storage_reason, record, Context, OpenPlan,
    Outcome,
};

/// How many times `today` looks for the day's file again after another
/// writer minted it between the look and the mint.
const TODAY_ATTEMPTS: usize = 2;

/// What a read verb holds: everything that reads and nothing that writes.
fn read_permissions() -> PermissionSet {
    [
        Capability::ListNotes,
        Capability::ReadNote,
        Capability::SearchNotes,
        Capability::ReadIndex,
    ]
    .into_iter()
    .collect()
}

/// What a write verb holds.
fn write_permissions() -> PermissionSet {
    [
        Capability::ReadNote,
        Capability::WriteNote,
        Capability::CreateNote,
    ]
    .into_iter()
    .collect()
}

/// The host over the notes folder and the index, holding `permissions` and
/// the data folder's write lock.
pub(crate) fn open_host(
    ctx: &Context,
    permissions: PermissionSet,
) -> Result<NoteHostImpl<'static>, Outcome> {
    NoteHostImpl::open(
        &ctx.notes_dir,
        Some(&ctx.db_path),
        permissions,
        ctx.default_extension,
    )
    .map(|host| host.with_write_lock(Some(WriteLock::in_data_folder(&ctx.writ_dir))))
    .map_err(|_| {
        Outcome::failed(format!(
            "the folder {} could not be read",
            ctx.notes_dir.display()
        ))
    })
}

/// The app's version store, keyed against the app's notes folder, for a write
/// into `served`; `None` when the file is not in the app's folder, which keeps
/// no version in the app either.
///
/// Opened only by a verb that writes, so a read leaves the data folder as it
/// found it, as `writ mcp` does (ADR-031 rule 1.3).
pub(crate) fn open_history(
    ctx: &Context,
    served: &Path,
    note: &str,
) -> Result<Option<NoteHistoryStore>, Outcome> {
    let Some(app_root) = ctx
        .app_notes_dir
        .as_deref()
        .and_then(resolve_for_containment)
        .filter(|root| root.is_dir())
    else {
        return Ok(None);
    };
    if !served.starts_with(&app_root) && !app_root.starts_with(served) {
        return Ok(None);
    }
    let store = NoteHistoryStore::open(&ctx.writ_dir).map_err(|_| {
        Outcome::failed(format!(
            "{note} was not changed: Writ could not open its version history to keep what the \
             file holds now."
        ))
    })?;
    store.set_probe(Arc::new(PlatformIdentity));
    store.set_notes_root(app_root);
    Ok(Some(store))
}

/// A host refusal as a sentence for a terminal.
pub(crate) fn host_failure(error: &HostError, ctx: &Context) -> String {
    match error {
        HostError::IndexUnavailable => match open_index(&ctx.db_path) {
            Err(why) => why.to_string(),
            Ok(_) => "the index could not be read".to_string(),
        },
        HostError::TooLarge { path, bytes } => {
            format!("{path} is {bytes} bytes, past the {MAX_NOTE_BYTES} bytes these commands read")
        }
        HostError::Conflict {
            path,
            conflict_copy: Some(copy),
        } => format!(
            "{path} changed on disk after it was read, so it was left alone. \
             The new text is in {copy}"
        ),
        HostError::Conflict {
            path,
            conflict_copy: None,
        } => format!("{path} changed on disk after it was read, so it was left alone"),
        other => other.to_string(),
    }
}

/// The absolute path of the file `arg` names: on disk first, then by name in
/// the index, which refuses a name two files answer to.
pub(crate) fn resolve_file(arg: &str, ctx: &Context) -> Result<PathBuf, String> {
    if let Some(path) = note_file(arg, ctx) {
        return Ok(path);
    }
    match open_index(&ctx.db_path) {
        Ok(store) => note_key(arg, &store, ctx).map(PathBuf::from),
        Err(_) => Err(format!("nothing called {arg}")),
    }
}

/// `writ search <query>`: files whose text matches, best match first.
pub fn search(query: &str, limit: usize, json: bool, ctx: &Context) -> Outcome {
    let host = match open_host(ctx, read_permissions()) {
        Ok(host) => host,
        Err(outcome) => return outcome,
    };
    let hits = match host.search_notes(query, 0, limit) {
        Ok(hits) => hits,
        Err(error) => return Outcome::failed(host_failure(&error, ctx)),
    };
    if json {
        let document = serde_json::json!({
            "query": query,
            "hits": hits.iter().map(|hit| serde_json::json!({
                "path": hit.path,
                "name": hit.name,
                "line": hit.line,
                "excerpt": hit.excerpt,
            })).collect::<Vec<_>>(),
        });
        return Outcome::ok(format!("{document}\n"));
    }
    let mut out = String::new();
    for hit in &hits {
        let line = hit.line.map(|line| line.to_string()).unwrap_or_default();
        out.push_str(&record([&hit.path, &line, &hit.excerpt]));
    }
    Outcome::ok(out)
}

/// `writ cat <file>`: the file's text, as written.
pub fn cat(arg: &str, json: bool, ctx: &Context) -> Outcome {
    let file = match resolve_file(arg, ctx) {
        Ok(file) => file,
        Err(message) => return Outcome::failed(message),
    };
    let host = match open_host(ctx, read_permissions()) {
        Ok(host) => host,
        Err(outcome) => return outcome,
    };
    match host.read_note(&file.to_string_lossy()) {
        Ok(content) if json => Outcome::ok(format!(
            "{}\n",
            serde_json::json!({
                "note": content.path,
                "bytes": content.bytes,
                "hash": content.hash,
                "text": content.text,
            })
        )),
        Ok(content) => Outcome::ok(content.text),
        Err(error) => Outcome::failed(host_failure(&error, ctx)),
    }
}

/// `writ ls [<folder>]`: every file in the notes folder, or in one folder
/// inside it, in path order.
pub fn ls(folder: Option<&str>, json: bool, ctx: &Context) -> Outcome {
    let host = match open_host(ctx, read_permissions()) {
        Ok(host) => host,
        Err(outcome) => return outcome,
    };
    let prefix = folder.map(|folder| folder.trim_start_matches("./"));
    let notes = match host.list_notes(prefix, usize::MAX) {
        Ok(notes) => notes,
        Err(error) => return Outcome::failed(host_failure(&error, ctx)),
    };
    if json {
        let document = serde_json::json!({
            "notes_folder": host.notes_root().to_string_lossy(),
            "notes": notes.iter().map(|note| serde_json::json!({
                "path": note.path,
                "name": note.name,
                "bytes": note.bytes,
            })).collect::<Vec<_>>(),
        });
        return Outcome::ok(format!("{document}\n"));
    }
    let mut out = String::new();
    for note in &notes {
        out.push_str(&record([&note.path, &note.name, &note.bytes.to_string()]));
    }
    Outcome::ok(out)
}

/// The text a writing verb was piped, or the usage error for none.
fn piped<'a>(verb: &str, ctx: &'a Context) -> Result<&'a str, Outcome> {
    ctx.stdin.as_deref().ok_or_else(|| {
        Outcome::usage(format!(
            "{verb} adds the text piped to it, and nothing is piped. Try: echo text | writ {verb}"
        ))
    })
}

/// A write receipt as a bare path or as the documented JSON document.
fn receipt_outcome(json: bool, receipt: &writ_core::notes::host::WriteReceipt) -> Outcome {
    if json {
        return Outcome::ok(format!(
            "{}\n",
            serde_json::json!({
                "note": receipt.path,
                "bytes": receipt.bytes,
                "hash": receipt.hash,
                "changed": receipt.changed,
            })
        ));
    }
    Outcome::ok(format!("{}\n", receipt.path))
}

/// `writ append <file>`: piped text added to the end of the file.
///
/// A name no file has yet is created in the notes folder holding the text, so
/// a script appending to one log needs no first step.
pub fn append(arg: &str, json: bool, ctx: &Context) -> Outcome {
    let text = match piped("append", ctx) {
        Ok(text) => text,
        Err(outcome) => return outcome,
    };
    let host = match open_host(ctx, write_permissions()) {
        Ok(host) => host,
        Err(outcome) => return outcome,
    };
    let file = match resolve_file(arg, ctx) {
        Ok(file) => file,
        Err(_) if is_bare_name(arg) => {
            let entry = writ_core::capture::capture_entry(text).unwrap_or_default();
            let content = writ_core::capture::append_entry("", &entry);
            return match host.create_note(arg, &content, WriteOrigin::Cli) {
                Ok(receipt) => receipt_outcome(json, &receipt),
                Err(error) => Outcome::failed(host_failure(&error, ctx)),
            };
        }
        Err(message) => return Outcome::failed(message),
    };
    let store = match open_history(ctx, host.notes_root(), arg) {
        Ok(store) => store,
        Err(outcome) => return outcome,
    };
    match host.with_history(store.as_ref()).append_note(
        &file.to_string_lossy(),
        text,
        WriteOrigin::Cli,
    ) {
        Ok(receipt) => receipt_outcome(json, &receipt),
        Err(error) => Outcome::failed(host_failure(&error, ctx)),
    }
}

/// Whether `arg` is a file name and not a path: nothing in it separates
/// folders.
fn is_bare_name(arg: &str) -> bool {
    !arg.trim().is_empty() && !arg.contains(['/', '\\'])
}

/// `writ today`: Today's File, minted when the day has none, with piped text
/// added to its end first under `--append`, then opened unless `--no-open`.
pub fn today(append: bool, open: bool, json: bool, ctx: &Context) -> Outcome {
    let text = if append {
        match piped("today --append", ctx) {
            Ok(text) => Some(text),
            Err(outcome) => return outcome,
        }
    } else {
        None
    };
    let host = match open_host(ctx, write_permissions()) {
        Ok(host) => host,
        Err(outcome) => return outcome,
    };
    // Opened only for an append, the one write here that replaces text.
    let store = match text {
        Some(_) => match open_history(ctx, host.notes_root(), "today's file") {
            Ok(store) => store,
            Err(outcome) => return outcome,
        },
        None => None,
    };
    let host = host.with_history(store.as_ref());
    let mut written = None;
    for _ in 0..TODAY_ATTEMPTS {
        let at = match find_todays_file(host.notes_root(), ctx.now, ctx.default_extension) {
            Ok(at) => at,
            Err(error) => return Outcome::failed(plain_storage_reason(&error)),
        };
        let attempt = match (at, text) {
            (TodaysFileAt::Existing(path), None) => Ok((path, false, false)),
            (TodaysFileAt::Existing(path), Some(text)) => host
                .append_note(&path.to_string_lossy(), text, WriteOrigin::Cli)
                .map(|receipt| (path, false, receipt.changed)),
            (TodaysFileAt::Missing(path), text) => {
                let name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let entry = text
                    .and_then(writ_core::capture::capture_entry)
                    .map(|entry| writ_core::capture::append_entry("", &entry))
                    .unwrap_or_default();
                let changed = !entry.is_empty();
                host.create_note(&name, &entry, WriteOrigin::Cli)
                    .map(|receipt| (PathBuf::from(receipt.path), true, changed))
            }
        };
        match attempt {
            Ok(done) => {
                written = Some(done);
                break;
            }
            Err(HostError::NameTaken { .. }) => continue,
            Err(error) => return Outcome::failed(host_failure(&error, ctx)),
        }
    }
    let Some((path, created, changed)) = written else {
        return Outcome::failed(
            "today's file was made by something else at the same moment; run it again".to_string(),
        );
    };
    let outcome = if json {
        Outcome::ok(format!(
            "{}\n",
            serde_json::json!({
                "note": path.to_string_lossy(),
                "created": created,
                "changed": changed,
            })
        ))
    } else {
        Outcome::ok(format!("{}\n", path.display()))
    };
    if !open {
        return outcome;
    }
    outcome.opening(OpenPlan {
        paths: vec![path],
        ..OpenPlan::default()
    })
}

/// `writ open <name>`: a file opened in Writ, at a line when the name ends in
/// `:LINE`, and waited on under `--wait`.
pub fn open(arg: &str, wait: bool, json: bool, ctx: &Context) -> Outcome {
    let (name, line) =
        writ_core::instance::split_line_target(arg, |whole| note_file(whole, ctx).is_some());
    let file = match resolve_file(&name, ctx) {
        Ok(file) => file,
        Err(message) => return Outcome::failed(message),
    };
    let mut plan = OpenPlan {
        paths: vec![file.clone()],
        wait,
        ..OpenPlan::default()
    };
    if let Some(line) = line {
        plan.lines.insert(file.to_string_lossy().into_owned(), line);
    }
    let outcome = if json {
        Outcome::ok(format!(
            "{}\n",
            serde_json::json!({ "note": file.to_string_lossy(), "line": line })
        ))
    } else {
        path_outcome(false, &file, None)
    };
    outcome.opening(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_name_holds_no_separator() {
        assert!(is_bare_name("Log"));
        assert!(is_bare_name("Log.md"));
        assert!(!is_bare_name("logs/Log.md"));
        assert!(!is_bare_name("..\\Log.md"));
        assert!(!is_bare_name("  "));
    }
}
