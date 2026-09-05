use crate::poison::recover_poison;
use crate::watcher::open_files::OpenNotes;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info};
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::budget::{Emission, EmissionBudget};
use writ_core::watcher::ignore::{IgnoreStamps, SuppressDecision, DEFAULT_IGNORE_TTL};
use writ_core::watcher::sighting::{FileSighting, LastSeen, DEFAULT_SIGHTING_TTL};

pub type IgnoreSet = Arc<Mutex<IgnoreStamps>>;

pub fn create_ignore_set() -> IgnoreSet {
    Arc::new(Mutex::new(IgnoreStamps::new()))
}

/// The path an ignore key is built from, for both the write that records the
/// stamp and the event that looks it up.
///
/// Both sides go through this one function because they have to agree
/// exactly: canonicalisation resolves symlinks and rewrites `/var` to
/// `/private/var`, so a stamp keyed by an unresolved path is a stamp no event
/// can ever match, and every save reads as somebody else's edit.
///
/// A file that does not exist yet resolves to its canonical folder plus its
/// name, which is the path the watcher delivers once the write creates it. A
/// path that resolves to nothing is used raw, which can only fail open: the
/// key misses and the event is emitted.
pub(crate) fn ignore_key_path(path: &Path) -> PathBuf {
    crate::security::resolve_for_containment(path)
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

/// What `path` holds, as its metadata describes it, or `None` when there is
/// no file there.
///
/// Metadata only, never a read: on Linux `notify` asks the kernel for `IN_OPEN`
/// on every folder it watches, so opening a file inside one raises another
/// event for it, and the read a watcher does to classify one event is what
/// delivers the next. Stat raises nothing, which is why every watcher here
/// asks this before it opens anything
/// ([`writ_core::watcher::sighting`]).
pub(crate) fn look_at(path: &Path) -> Option<FileSighting> {
    let metadata = std::fs::metadata(path).ok()?;
    Some(FileSighting {
        len: metadata.len(),
        modified: metadata.modified().ok(),
    })
}

/// Opaque owner of the file watcher's debouncer.
///
/// Held by `AppState` so the watcher lives as long as the application.
/// Dropping this handle drops the inner `Debouncer`, which closes the
/// event channel and causes the watcher thread to exit cleanly.
pub struct WatcherHandle {
    _debouncer: Debouncer<RecommendedWatcher>,
}

/// Watches the config file.
///
/// It no longer watches Writ's data folder: nothing there holds note text
/// after ADR-028 §1, so an event from it can only be noise. A note's own file
/// is watched from release 0.5.
pub fn start_file_watcher(
    bus: Arc<EventBus>,
    config_path: PathBuf,
    ignore_set: IgnoreSet,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let ttl = DEFAULT_IGNORE_TTL;
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;

    if config_path.exists() {
        debouncer
            .watcher()
            .watch(&config_path, RecursiveMode::NonRecursive)?;
    }

    info!("file watcher started");

    std::thread::spawn(move || {
        let mut seen = LastSeen::new();
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = report_config_event(
                            &event.path,
                            &config_path,
                            &ignore_set,
                            &mut seen,
                            ttl,
                            Instant::now(),
                        ) {
                            bus.emit(domain_event);
                        }
                    }
                }
                Err(e) => {
                    error!("watcher error: {:?}", e);
                }
            }
        }
        info!("watcher thread exiting");
    });

    Ok(WatcherHandle {
        _debouncer: debouncer,
    })
}

/// Starts a recursive watcher on the workspace `root`, emitting
/// [`WritEvent::WorkspaceChanged`] for surfaced paths.
///
/// Saving a buffer writes the file it was opened from, so Writ's own writes
/// do land inside a watched workspace. They are not suppressed: the only
/// consumer is the file index, which patches the one path in place, and an
/// upsert of a path it already holds changes nothing. Fingerprinting every
/// event to skip a no-op would cost a read of the file per keystroke burst.
pub fn start_workspace_watcher(
    bus: Arc<EventBus>,
    root: PathBuf,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    info!(root = %root.display(), "workspace watcher started");

    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = classify_workspace_event(&event.path, &root) {
                            bus.emit(domain_event);
                        }
                    }
                }
                Err(e) => {
                    error!("workspace watcher error: {:?}", e);
                }
            }
        }
        info!("workspace watcher thread exiting");
    });

    Ok(WatcherHandle {
        _debouncer: debouncer,
    })
}

/// Starts a recursive watcher on the inbox `root`, emitting
/// [`WritEvent::InboxFileArrived`] for qualifying files created after the
/// watcher started (ADR-018).
///
/// Saving a buffer writes the file it was opened from, so an inbox file the
/// user is editing is written inside the watched tree: the ignore set is
/// consulted to keep Writ's own saves from arriving as new files, which would
/// reopen the tab and pull the window forward mid-keystroke.
pub fn start_inbox_watcher(
    bus: Arc<EventBus>,
    root: PathBuf,
    ignore_set: IgnoreSet,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    // Snapshot AFTER the watch is registered: a file landing during the scan
    // is suppressed (it is in the snapshot) rather than double-reported, and
    // nothing created after the scan can be missed.
    let preexisting = snapshot_files(&root);

    info!(root = %root.display(), preexisting = preexisting.len(), "inbox watcher started");

    std::thread::spawn(move || {
        let mut seen = LastSeen::new();
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = report_inbox_event(
                            &event.path,
                            &root,
                            &preexisting,
                            &ignore_set,
                            &mut seen,
                            DEFAULT_IGNORE_TTL,
                            Instant::now(),
                        ) {
                            bus.emit(domain_event);
                        }
                    }
                }
                Err(e) => {
                    error!("inbox watcher error: {:?}", e);
                }
            }
        }
        info!("inbox watcher thread exiting");
    });

    Ok(WatcherHandle {
        _debouncer: debouncer,
    })
}

/// Collects every regular file under `root`, recursively. The snapshot is
/// the arrival discriminator (ADR-024): events for paths in this set are
/// pre-existing backlog or modifications, never arrivals.
fn snapshot_files(root: &Path) -> std::collections::HashSet<PathBuf> {
    let mut files = std::collections::HashSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            match entry.file_type() {
                Ok(t) if t.is_dir() => stack.push(path),
                Ok(t) if t.is_file() => {
                    files.insert(path);
                }
                _ => {}
            }
        }
    }
    files
}

/// What one *delivered* inbox event is worth saying, or nothing.
///
/// The watcher thread's entry point, for the reason
/// [`report_notes_event`] gives: classifying an arrival reads the file to
/// fingerprint it and again to decide how it would open, and on Linux each of
/// those reads is another event for the same path. Left unguarded, one file
/// landing in the inbox reopens its tab for as long as the app runs.
pub fn report_inbox_event(
    path: &Path,
    root: &Path,
    preexisting: &std::collections::HashSet<PathBuf>,
    ignore_set: &IgnoreSet,
    seen: &mut LastSeen,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if !seen.is_news(path, look_at(path), now, DEFAULT_SIGHTING_TTL) {
        return None;
    }
    classify_inbox_event(path, root, preexisting, ignore_set, ttl, now)
}

/// Classifies an inbox file-system event into [`WritEvent::InboxFileArrived`],
/// or suppresses it.
///
/// Mechanism only: reads file metadata, then defers the auto-open decision
/// to `writ_core::inbox::qualifies_for_auto_open` (containment, ignore set,
/// snapshot membership) and `file_ops::classify_path`. Only files that
/// classify as [`FileOpenMode::Normal`] auto-open: large-file-mode and binary
/// (hex) buffers disable the rendered view the inbox exists to show. The
/// debouncer does not distinguish create from modify, so the snapshot taken
/// at watch start is the discriminator: an event for a path in the snapshot
/// is a pre-existing file (possibly modified) and is suppressed (ADR-024).
///
/// A file Writ has just saved is its own write, not an arrival: the ignore
/// set is keyed by the file's canonical path under the source namespace and
/// fingerprints the bytes, so suppression here cannot swallow someone else's
/// edit to the same file, nor a change to a file of the same name in another
/// folder.
pub fn classify_inbox_event(
    path: &Path,
    root: &Path,
    preexisting: &std::collections::HashSet<PathBuf>,
    ignore_set: &IgnoreSet,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if !writ_core::inbox::qualifies_for_auto_open(root, path, preexisting) {
        return None;
    }
    let key = writ_core::watcher::ignore::source_key(&ignore_key_path(path));
    let decision = {
        let current_bytes = std::fs::read(path).ok();
        let mut set = recover_poison(ignore_set.lock(), "watcher::handler::inbox_event");
        set.decide(&key, current_bytes.as_deref(), now, ttl)
    };
    if decision == SuppressDecision::Suppress {
        return None;
    }
    match writ_core::file_ops::classify_path(path) {
        Ok(c) if c.mode == writ_core::file_ops::FileOpenMode::Normal => {}
        _ => return None,
    }
    info!(file = %path.display(), "inbox file arrived");
    Some(WritEvent::InboxFileArrived {
        path: path.to_string_lossy().into_owned(),
    })
}

/// Starts a recursive watcher on the notes `root`, emitting
/// [`WritEvent::NotesChanged`] for a file another program created, changed or
/// removed there, and [`WritEvent::BufferExternal`] as well when that file is
/// open in a tab.
///
/// This keeps the index honest about a folder the user also edits from
/// Obsidian, a phone, or a sync client, and it is the whole route by which a
/// note *inside* the notes folder tells its tab that somebody else rewrote it.
/// Files opened from anywhere else take the same news through
/// [`super::open_files`], which is why the two build the event with the same
/// function: which folder a file happens to sit in must not change what its
/// tab is told.
///
/// Writ's own saves are stamped into `ignore_set` before they land, so they do
/// not arrive back here as somebody else's edit; the store indexes them itself.
///
/// How much one window may say about the folder is capped ([`EmissionBudget`]).
/// A sync client catching up, or a plugin another editor left running,
/// rewrites hundreds of files inside a single debounce window; naming each one
/// is a message the frontend has to receive and act on. Over the cap the
/// watcher stops naming files and emits [`WritEvent::NotesSwept`] once, and
/// every listener re-checks what it holds. The budget is spent only on changes
/// that survived classification, so a burst of Writ's own saves cannot make the
/// folder look like it moved.
///
/// Telling an open tab is outside that cap and bounded another way: by how many
/// tabs are open, deduplicated per batch, so a folder being churned through
/// costs at most one message per open tab per window however many files moved.
/// Capping it would mean a tab losing the one change its user cares about
/// because five hundred files they have never opened moved in the same second.
pub fn start_notes_watcher(
    bus: Arc<EventBus>,
    root: PathBuf,
    ignore_set: IgnoreSet,
    open_notes: Arc<dyn OpenNotes>,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    info!(root = %root.display(), "notes watcher started");

    std::thread::spawn(move || {
        let mut budget = EmissionBudget::new();
        // Outside the batch loop: the event a read of this watcher's own
        // raises on Linux arrives in a later batch than the change that
        // caused it, so a record scoped to one batch would never see it.
        let mut seen = LastSeen::new();
        loop {
            // A change the budget dropped was covered by a sweep that had
            // already gone out, and the walk that sweep started may have read
            // the file before it changed. If the folder then falls quiet,
            // nothing else will ever raise it, so the wait ends at the moment
            // that sweep stops standing and the folder is swept once more.
            let result = match budget.owed_sweep_at() {
                Some(due) => match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(result) => result,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        if budget.take_owed_sweep(Instant::now()) {
                            info!(
                                root = %root.display(),
                                "the notes folder fell quiet mid-sweep; sweeping once more"
                            );
                            bus.emit(notes_swept(&root));
                        }
                        continue;
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(result) => result,
                    Err(_) => break,
                },
            };
            match result {
                Ok(events) => {
                    // One tab message per note per delivered batch, the same
                    // rule the open-file watcher runs on.
                    let mut told: HashSet<String> = HashSet::new();
                    for event in events {
                        let now = Instant::now();
                        let Some(domain_event) = report_notes_event(
                            &event.path,
                            &root,
                            &ignore_set,
                            &mut seen,
                            DEFAULT_IGNORE_TTL,
                            now,
                        ) else {
                            continue;
                        };
                        if let Some(for_tab) = route_notes_change_to_open_tab(
                            &domain_event,
                            open_notes.as_ref(),
                            &mut told,
                        ) {
                            bus.emit(for_tab);
                        }
                        match budget.admit(now) {
                            Emission::Name => bus.emit(domain_event),
                            Emission::Sweep => {
                                info!(
                                    root = %root.display(),
                                    "notes folder changed faster than it can be listed; sweeping"
                                );
                                bus.emit(notes_swept(&root));
                            }
                            Emission::Drop => {}
                        }
                    }
                }
                Err(e) => {
                    error!("notes watcher error: {:?}", e);
                }
            }
        }
        info!("notes watcher thread exiting");
    });

    Ok(WatcherHandle {
        _debouncer: debouncer,
    })
}

/// The tab event a classified notes change becomes, when the file it names is
/// open and has not already been reported in this batch.
///
/// `told` is the batch's record of which notes have been named, and this
/// updates it. Splitting the decision out of the watcher thread is what makes
/// it testable without a filesystem: the caller supplies the lookup and the
/// batch state.
pub fn route_notes_change_to_open_tab(
    event: &WritEvent,
    open_notes: &dyn OpenNotes,
    told: &mut HashSet<String>,
) -> Option<WritEvent> {
    let WritEvent::NotesChanged { path, removed } = event else {
        return None;
    };
    let path = Path::new(path);
    let note_id = open_notes.note_at(path)?;
    if !told.insert(note_id.clone()) {
        return None;
    }
    Some(super::open_files::open_note_change(
        &note_id, path, *removed,
    ))
}

/// The event that stands for "more changed in the notes folder than is worth
/// listing".
pub fn notes_swept(root: &Path) -> WritEvent {
    WritEvent::NotesSwept {
        root: root.to_string_lossy().into_owned(),
    }
}

/// What one *delivered* notes event is worth saying, or nothing.
///
/// This, rather than [`classify_notes_event`], is what the watcher thread
/// calls, and the two are separate so that the record of what has already been
/// looked at cannot be skipped by the caller that matters. An event describing
/// the file exactly as this watcher last found it is dropped before anything
/// opens it, which is what keeps a classification's own read from arriving
/// back as the next change on Linux ([`writ_core::watcher::sighting`]).
pub fn report_notes_event(
    path: &Path,
    root: &Path,
    ignore_set: &IgnoreSet,
    seen: &mut LastSeen,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if !seen.is_news(path, look_at(path), now, DEFAULT_SIGHTING_TTL) {
        return None;
    }
    classify_notes_event(path, root, ignore_set, ttl, now)
}

/// Classifies a notes-folder event into a domain event, or suppresses it.
///
/// Suppressed: a path outside `root`, a path under a folder another client
/// left behind (`.obsidian`, `.trash`, `.stfolder`, `.stversions`), a name
/// `writ_core::workspace::is_ignored_name` answers for — the temp file every
/// atomic write creates beside its target, a sync client's in-flight file, an
/// undownloaded placeholder, an editor swap file — and a write Writ itself
/// made, which the ignore set recognises by canonical path and content
/// fingerprint under the source namespace (ADR-028 section 6).
///
/// Filtering those names is a correctness rule rather than a tidiness one:
/// `write_atomic` persists through a `NamedTempFile` created beside its target,
/// so every internal save fans out into a create-and-delete pair for a `.tmp`
/// path, and a watcher over a folder Writ writes into has to drop them before
/// it emits anything. A sync client catching up fans out the same way over its
/// own temp names.
pub fn classify_notes_event(
    path: &Path,
    root: &Path,
    ignore_set: &IgnoreSet,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if !path.starts_with(root) {
        return None;
    }
    if writ_core::workspace::path_has_ignored_name(root, path) {
        return None;
    }
    // A path with no file name is the root itself, which is not a change to a
    // note.
    path.file_name()?;

    let removed = !path.exists();
    if !removed && !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return None;
    }

    let key = writ_core::watcher::ignore::source_key(&ignore_key_path(path));
    let decision = {
        let current_bytes = std::fs::read(path).ok();
        let mut set = recover_poison(ignore_set.lock(), "watcher::handler::notes_event");
        set.decide(&key, current_bytes.as_deref(), now, ttl)
    };
    if decision == SuppressDecision::Suppress {
        return None;
    }

    Some(WritEvent::NotesChanged {
        path: path.to_string_lossy().into_owned(),
        removed,
    })
}

/// Classifies a workspace file-system event into a domain event, or
/// suppresses it when the path is outside `root` or sits under an
/// ignored directory.
pub fn classify_workspace_event(path: &Path, root: &Path) -> Option<WritEvent> {
    if !path.starts_with(root) {
        return None;
    }
    if writ_core::workspace::path_has_ignored_component(root, path) {
        return None;
    }
    Some(WritEvent::WorkspaceChanged {
        path: path.to_string_lossy().into_owned(),
        removed: !path.exists(),
    })
}

/// What one *delivered* config event is worth saying, or nothing.
///
/// The watcher thread's entry point, for the reason [`report_notes_event`]
/// gives, and the config file is the worst of the three: the read that
/// fingerprints it clears the stamp on its way out, so on Linux an edit made
/// in another editor announced itself over and over and the frontend reloaded
/// the config each time.
pub fn report_config_event(
    path: &Path,
    config_path: &Path,
    ignore_set: &IgnoreSet,
    seen: &mut LastSeen,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if path != config_path {
        return None;
    }
    if !seen.is_news(path, look_at(path), now, DEFAULT_SIGHTING_TTL) {
        return None;
    }
    classify_watch_event(path, config_path, ignore_set, ttl, now)
}

/// Classifies a single file-system event into a domain event, or suppresses
/// it. Pure aside from a single `fs::read` to fingerprint the file against
/// the ignore set; callers test it directly with a tempdir.
///
/// The config file has its own key namespace: a note named `config.toml` that
/// Writ saves must not suppress a real config reload (ADR-028 section 6).
///
/// Only the config file qualifies. Anything else is dropped, and dropping it
/// early is a correctness rule rather than a tidiness one: `write_atomic`
/// persists through a `NamedTempFile` created beside its target, so every
/// internal save emits a create-and-delete pair for a `.tmp*` path. Turning
/// one of those into a change event makes the frontend reload the document
/// registry, which tears down and recreates an open `writ-preview://` iframe
/// mid-edit, and removing a loaded one hard-freezes the macOS webview. Any
/// watcher added over a folder Writ writes into has to filter its own temp
/// files before it emits anything.
///
/// [`report_config_event`] is what the watcher thread calls; this is the
/// classification on its own.
pub fn classify_watch_event(
    path: &Path,
    config_path: &Path,
    ignore_set: &IgnoreSet,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if path != config_path {
        return None;
    }

    let key = writ_core::watcher::ignore::config_key(&ignore_key_path(config_path));

    let current_bytes = std::fs::read(path).ok();
    let decision = {
        let mut set = recover_poison(ignore_set.lock(), "watcher::handler::config_event");
        set.decide(&key, current_bytes.as_deref(), now, ttl)
    };

    if decision == SuppressDecision::Suppress {
        return None;
    }

    info!("config file changed");
    Some(WritEvent::ConfigChanged {
        keys: vec!["*".to_string()],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::fs;
    use tempfile::tempdir;

    fn make_set() -> IgnoreSet {
        create_ignore_set()
    }

    /// The key a save of `path` records, built the way the command layer
    /// builds it.
    fn source_stamp_key(path: &Path) -> String {
        writ_core::watcher::ignore::source_key(&ignore_key_path(path))
    }

    /// The key a config write records.
    fn config_stamp_key(path: &Path) -> String {
        writ_core::watcher::ignore::config_key(&ignore_key_path(path))
    }

    /// Inbox classification with an empty ignore set — the case where every
    /// write is somebody else's.
    fn classify_inbox(
        path: &Path,
        root: &Path,
        preexisting: &std::collections::HashSet<PathBuf>,
    ) -> Option<WritEvent> {
        classify_inbox_event(
            path,
            root,
            preexisting,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        )
    }

    #[test]
    fn notify_keeps_default_backends() {
        // #55: pinning notify to `default-features = false, features =
        // ["macos_fsevent"]` drops the inotify / ReadDirectoryChangesW backends
        // the moment a transitive consumer stops unifying them back in, silently
        // downgrading Linux/Windows watching to PollWatcher. Lock the dependency
        // to notify's default backend set. `recommended_watcher_resolves_to_native_backend`
        // is the runtime complement; this is the cheap manifest guard that fails
        // on macOS the instant the mac-only feature pin returns.
        let manifest = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
        )
        .expect("read src-tauri/Cargo.toml");
        let notify_line = manifest
            .lines()
            .find(|l| {
                let t = l.trim_start();
                t.starts_with("notify ") || t.starts_with("notify=")
            })
            .expect("notify dependency declared in src-tauri/Cargo.toml");
        assert!(
            !notify_line.contains("default-features = false"),
            "notify must keep default features so every target's native watcher backend compiles in (#55): {notify_line}"
        );
    }

    #[test]
    fn recommended_watcher_resolves_to_native_backend() {
        // notify's RecommendedWatcher is a per-platform type alias that only
        // resolves to the native backend (FSEvents/inotify/ReadDirectoryChangesW)
        // when that backend's cargo feature is compiled in. Pin notify to a
        // mac-only feature set and it silently falls back to PollWatcher on
        // Linux/Windows — cross-platform watching degrades to polling with no
        // build error. Assert the resolved backend is never the poll fallback.
        use notify::{Watcher, WatcherKind};
        assert_ne!(
            <RecommendedWatcher as Watcher>::kind(),
            WatcherKind::PollWatcher,
            "notify resolved to PollWatcher: a native filesystem backend is missing for this target"
        );
    }

    #[test]
    fn the_events_one_write_raises_on_linux_report_the_change_once() {
        // A rename-over inside the notes folder fans out into several raw
        // inotify events, and classifying the first opens the file, which
        // raises another for the same path, whose classification opens it
        // again. Delivered one per batch, that reached the bus as eleven
        // identical BufferExternal events on CI. The record of what has
        // already been looked at is what ends it, so it is held across the
        // batches the way the watcher thread holds it.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let note = root.join("today.md");
        fs::write(&note, b"rewritten by another program\n").unwrap();

        let ignore = make_set();
        let now = Instant::now();
        let ungated = (0..11)
            .filter(|_| {
                classify_notes_event(&note, root, &make_set(), DEFAULT_IGNORE_TTL, now).is_some()
            })
            .count();
        assert_eq!(
            ungated, 11,
            "the burst is what classification alone reports"
        );

        let mut seen = LastSeen::new();
        let reported: Vec<WritEvent> = (0..11)
            .filter_map(|_| {
                report_notes_event(&note, root, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now)
            })
            .collect();

        assert_eq!(
            reported.len(),
            1,
            "one write must be reported once, saw {reported:?}"
        );
    }

    #[test]
    fn a_second_write_is_reported_however_recently_the_first_was() {
        // The record must not swallow a real change: the file is written
        // again, so it is not the file this watcher last looked at.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let note = root.join("today.md");
        fs::write(&note, b"first\n").unwrap();

        let ignore = make_set();
        let mut seen = LastSeen::new();
        let now = Instant::now();
        assert!(
            report_notes_event(&note, root, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_some()
        );
        assert!(
            report_notes_event(&note, root, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_none()
        );

        fs::write(&note, b"second, and longer\n").unwrap();
        assert!(
            report_notes_event(&note, root, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_some(),
            "a file written again must reach the folder and the tab"
        );
    }

    #[test]
    fn a_note_that_has_gone_is_reported_once() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let note = root.join("today.md");
        fs::write(&note, b"x\n").unwrap();
        fs::remove_file(&note).unwrap();

        let ignore = make_set();
        let mut seen = LastSeen::new();
        let now = Instant::now();
        let reported: Vec<WritEvent> = (0..11)
            .filter_map(|_| {
                report_notes_event(&note, root, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now)
            })
            .collect();

        assert_eq!(reported.len(), 1, "saw {reported:?}");
        match &reported[0] {
            WritEvent::NotesChanged { removed, .. } => assert!(removed),
            other => panic!("expected NotesChanged, got {other:?}"),
        }
    }

    #[test]
    fn the_events_one_arrival_raises_on_linux_open_the_tab_once() {
        // Classifying an arrival reads the file twice, to fingerprint it and
        // to decide how it would open, and an arrival is never in the
        // start-of-run snapshot that would otherwise suppress it. Unguarded,
        // one file landing in the inbox reopens its tab for as long as the
        // app runs.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let arrival = root.join("report.md");
        fs::write(&arrival, b"# done\n").unwrap();

        let preexisting = std::collections::HashSet::new();
        let ignore = make_set();
        let now = Instant::now();
        let ungated = (0..11)
            .filter(|_| {
                classify_inbox_event(
                    &arrival,
                    root,
                    &preexisting,
                    &make_set(),
                    DEFAULT_IGNORE_TTL,
                    now,
                )
                .is_some()
            })
            .count();
        assert_eq!(
            ungated, 11,
            "the burst is what classification alone reports"
        );

        let mut seen = LastSeen::new();
        let reported: Vec<WritEvent> = (0..11)
            .filter_map(|_| {
                report_inbox_event(
                    &arrival,
                    root,
                    &preexisting,
                    &ignore,
                    &mut seen,
                    DEFAULT_IGNORE_TTL,
                    now,
                )
            })
            .collect();

        assert_eq!(
            reported.len(),
            1,
            "one arrival must open its tab once, saw {reported:?}"
        );
        assert!(matches!(reported[0], WritEvent::InboxFileArrived { .. }));
    }

    #[test]
    fn a_second_arrival_is_reported_however_recently_the_first_was() {
        let dir = tempdir().unwrap();
        let root = dir.path();
        let first = root.join("report.md");
        let second = root.join("notes.md");
        fs::write(&first, b"# done\n").unwrap();
        fs::write(&second, b"# other\n").unwrap();

        let preexisting = std::collections::HashSet::new();
        let ignore = make_set();
        let mut seen = LastSeen::new();
        let now = Instant::now();
        let report = |path: &Path, seen: &mut LastSeen| {
            report_inbox_event(
                path,
                root,
                &preexisting,
                &ignore,
                seen,
                DEFAULT_IGNORE_TTL,
                now,
            )
        };
        assert!(report(&first, &mut seen).is_some());
        assert!(report(&first, &mut seen).is_none());
        assert!(
            report(&second, &mut seen).is_some(),
            "a second file landing in the inbox must open its own tab"
        );
    }

    #[test]
    fn the_events_one_config_edit_raises_on_linux_reload_it_once() {
        // The config file is the worst of the three: the read that
        // fingerprints it clears the stamp on its way out, so nothing else
        // stops an external edit announcing itself on every turn of the loop.
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"theme = \"dark\"\n").unwrap();

        let ignore = make_set();
        let now = Instant::now();
        {
            let mut guard = ignore.lock().unwrap();
            guard.record(config_stamp_key(&cfg), b"theme = \"dark\"\n", now);
        }
        fs::write(&cfg, b"theme = \"light\"\n").unwrap();

        let ungated = (0..11)
            .filter(|_| {
                classify_watch_event(&cfg, &cfg, &ignore, DEFAULT_IGNORE_TTL, now).is_some()
            })
            .count();
        assert_eq!(
            ungated, 11,
            "the stamp is cleared on the way out, so classification alone reports every turn"
        );

        let mut seen = LastSeen::new();
        let reported: Vec<WritEvent> = (0..11)
            .filter_map(|_| {
                report_config_event(&cfg, &cfg, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now)
            })
            .collect();

        assert_eq!(
            reported.len(),
            1,
            "one config edit must reload the config once, saw {reported:?}"
        );
        assert!(matches!(reported[0], WritEvent::ConfigChanged { .. }));
    }

    #[test]
    fn a_config_edit_is_reported_however_recently_the_last_one_was() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"theme = \"dark\"\n").unwrap();

        let ignore = make_set();
        let mut seen = LastSeen::new();
        let now = Instant::now();
        assert!(
            report_config_event(&cfg, &cfg, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_some()
        );
        assert!(
            report_config_event(&cfg, &cfg, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_none()
        );

        fs::write(&cfg, b"theme = \"light\", font_size = 15\n").unwrap();
        assert!(
            report_config_event(&cfg, &cfg, &ignore, &mut seen, DEFAULT_IGNORE_TTL, now).is_some(),
            "an edit made after the last look must still reload the config"
        );
    }

    #[test]
    fn a_sweep_is_its_own_event_and_names_the_folder() {
        // A listener discriminates on the variant rather than comparing a path
        // against a root it would have to fetch and normalise itself.
        let dir = tempdir().unwrap();
        let root = dir.path();

        match notes_swept(root) {
            WritEvent::NotesSwept { root: named } => {
                assert_eq!(named, root.to_string_lossy());
            }
            other => panic!("expected NotesSwept, got {other:?}"),
        }
    }

    /// A fixed set of open notes, so the routing can be tested without opening
    /// a file or standing up a registry.
    struct FixedNotes(HashMap<PathBuf, String>);

    impl OpenNotes for FixedNotes {
        fn note_at(&self, path: &Path) -> Option<String> {
            self.0.get(path).cloned()
        }
    }

    fn open_as(path: &Path, note_id: &str) -> FixedNotes {
        FixedNotes(HashMap::from([(path.to_path_buf(), note_id.to_string())]))
    }

    #[test]
    fn a_change_to_a_note_that_is_open_is_routed_to_its_tab() {
        // The core of W1 on the folder that holds nearly every note. A change
        // in the notes folder used to reach the index and stop there, so a tab
        // showed text its file no longer held until it was closed and
        // reopened.
        let dir = tempdir().unwrap();
        let note = dir.path().join("today.md");
        fs::write(&note, b"rewritten by another program").unwrap();

        let change = WritEvent::NotesChanged {
            path: note.to_string_lossy().into_owned(),
            removed: false,
        };
        let mut told = HashSet::new();

        match route_notes_change_to_open_tab(&change, &open_as(&note, "note-1"), &mut told) {
            Some(WritEvent::BufferExternal {
                buffer_id,
                path,
                change,
                disk_hash,
                new_path,
            }) => {
                assert_eq!(buffer_id, "note-1");
                assert_eq!(path, note.to_string_lossy());
                assert_eq!(
                    change,
                    writ_core::watcher::change_event::ExternalChange::Modified
                );
                assert_eq!(
                    disk_hash.as_deref(),
                    Some(
                        writ_core::hash::comparison_digest_hex(b"rewritten by another program")
                            .as_str()
                    ),
                    "the tab compares its document against this digest, so it has to be the file's"
                );
                assert_eq!(new_path, None);
            }
            other => panic!("expected BufferExternal, got {other:?}"),
        }
    }

    #[test]
    fn a_change_to_a_note_nobody_has_open_is_routed_nowhere() {
        let dir = tempdir().unwrap();
        let note = dir.path().join("today.md");
        let unopened = dir.path().join("archive.md");
        fs::write(&note, b"x").unwrap();
        fs::write(&unopened, b"x").unwrap();

        let change = WritEvent::NotesChanged {
            path: unopened.to_string_lossy().into_owned(),
            removed: false,
        };
        let mut told = HashSet::new();

        assert!(
            route_notes_change_to_open_tab(&change, &open_as(&note, "note-1"), &mut told).is_none()
        );
    }

    #[test]
    fn a_deleted_note_tells_its_tab_the_file_is_gone() {
        let dir = tempdir().unwrap();
        let note = dir.path().join("today.md");

        let change = WritEvent::NotesChanged {
            path: note.to_string_lossy().into_owned(),
            removed: true,
        };
        let mut told = HashSet::new();

        match route_notes_change_to_open_tab(&change, &open_as(&note, "note-1"), &mut told) {
            Some(WritEvent::BufferExternal {
                buffer_id,
                change,
                disk_hash,
                ..
            }) => {
                assert_eq!(buffer_id, "note-1");
                assert_eq!(
                    change,
                    writ_core::watcher::change_event::ExternalChange::Deleted
                );
                assert_eq!(disk_hash, None, "there is nothing left to hash");
            }
            other => panic!("expected BufferExternal, got {other:?}"),
        }
    }

    #[test]
    fn one_batch_tells_a_tab_once_however_often_its_file_was_written() {
        // A program rewriting a file in a loop lands several events in one
        // debounce batch. The tab needs the news once.
        let dir = tempdir().unwrap();
        let note = dir.path().join("today.md");
        fs::write(&note, b"x").unwrap();

        let change = WritEvent::NotesChanged {
            path: note.to_string_lossy().into_owned(),
            removed: false,
        };
        let open = open_as(&note, "note-1");
        let mut told = HashSet::new();

        assert!(route_notes_change_to_open_tab(&change, &open, &mut told).is_some());
        for _ in 0..10 {
            assert!(route_notes_change_to_open_tab(&change, &open, &mut told).is_none());
        }

        // A new batch starts a new record, because the file may well have
        // changed again.
        let mut next_batch = HashSet::new();
        assert!(route_notes_change_to_open_tab(&change, &open, &mut next_batch).is_some());
    }

    #[test]
    fn a_sweep_is_not_routed_to_any_tab() {
        // The sweep says the folder moved, not that any one file did. The
        // frontend re-checks every open file on it; turning it into a change
        // to whichever note happens to sit at the root path would be a claim
        // about a file nothing looked at.
        let dir = tempdir().unwrap();
        let note = dir.path().join("today.md");
        fs::write(&note, b"x").unwrap();
        let mut told = HashSet::new();

        assert!(route_notes_change_to_open_tab(
            &notes_swept(dir.path()),
            &open_as(&note, "note-1"),
            &mut told
        )
        .is_none());
    }

    #[test]
    fn a_note_event_can_never_carry_the_folder_itself() {
        // A directory is not a note change, whichever folder it is.
        let dir = tempdir().unwrap();
        let root = dir.path();
        let sub = root.join("archive");
        fs::create_dir(&sub).unwrap();

        assert!(
            classify_notes_event(root, root, &make_set(), DEFAULT_IGNORE_TTL, Instant::now())
                .is_none(),
            "the notes root must never surface as a note change"
        );
        assert!(
            classify_notes_event(&sub, root, &make_set(), DEFAULT_IGNORE_TTL, Instant::now())
                .is_none(),
            "a folder inside the notes root must never surface as a note change"
        );
    }

    #[test]
    fn classifies_config_path_as_config_changed() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"x").unwrap();

        let event =
            classify_watch_event(&cfg, &cfg, &make_set(), DEFAULT_IGNORE_TTL, Instant::now());

        assert!(matches!(event, Some(WritEvent::ConfigChanged { .. })));
    }

    #[test]
    fn suppresses_internal_config_write() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let bytes = b"theme = \"dark\"\n";
        fs::write(&cfg, bytes).unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(config_stamp_key(&cfg), bytes, now);
        }

        let event = classify_watch_event(&cfg, &cfg, &set, DEFAULT_IGNORE_TTL, now);

        assert!(event.is_none(), "internal config write must be suppressed");
    }

    #[test]
    fn emits_external_config_change_when_bytes_differ() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"theme = \"dark\"\n").unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(config_stamp_key(&cfg), b"theme = \"dark\"\n", now);
        }

        fs::write(&cfg, b"theme = \"light\"\n").unwrap();

        let event = classify_watch_event(&cfg, &cfg, &set, DEFAULT_IGNORE_TTL, now);

        assert!(
            matches!(event, Some(WritEvent::ConfigChanged { .. })),
            "an external config edit must surface as ConfigChanged"
        );
    }

    #[test]
    fn workspace_event_inside_root_surfaces_with_removed_state() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("notes.md");
        fs::write(&file, b"x").unwrap();

        match classify_workspace_event(&file, dir.path()) {
            Some(WritEvent::WorkspaceChanged { path, removed }) => {
                assert_eq!(path, file.to_string_lossy());
                assert!(!removed);
            }
            other => panic!("expected WorkspaceChanged, got {:?}", other),
        }

        fs::remove_file(&file).unwrap();
        match classify_workspace_event(&file, dir.path()) {
            Some(WritEvent::WorkspaceChanged { removed, .. }) => assert!(removed),
            other => panic!("expected WorkspaceChanged removed, got {:?}", other),
        }
    }

    #[test]
    fn workspace_event_under_ignored_dir_is_suppressed() {
        let dir = tempdir().unwrap();
        let inside = dir.path().join("node_modules").join("pkg").join("a.js");

        assert!(classify_workspace_event(&inside, dir.path()).is_none());
    }

    #[test]
    fn workspace_event_outside_root_is_suppressed() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        let outside = other.path().join("a.txt");

        assert!(classify_workspace_event(&outside, dir.path()).is_none());
    }

    #[test]
    fn ignores_every_path_that_is_not_the_config_file() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let unrelated = dir.path().join("unrelated.log");
        fs::write(&unrelated, b"x").unwrap();

        let event = classify_watch_event(
            &unrelated,
            &cfg,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );

        assert!(event.is_none());
    }

    #[test]
    fn inbox_event_for_file_absent_from_snapshot_surfaces() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("report.md");
        fs::write(&file, b"# done").unwrap();

        match classify_inbox(&file, dir.path(), &preexisting) {
            Some(WritEvent::InboxFileArrived { path }) => {
                assert_eq!(path, file.to_string_lossy());
            }
            other => panic!("expected InboxFileArrived, got {:?}", other),
        }
    }

    #[test]
    fn inbox_event_for_preexisting_file_modified_later_is_suppressed() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("old.md");
        fs::write(&file, b"before").unwrap();

        let preexisting: std::collections::HashSet<_> = [file.clone()].into_iter().collect();
        fs::write(&file, b"after").unwrap();

        assert!(
            classify_inbox(&file, dir.path(), &preexisting).is_none(),
            "a file present at watch start must never auto-open"
        );
    }

    #[test]
    fn inbox_event_under_ignored_dir_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let nested = dir.path().join("node_modules").join("pkg");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("readme.md");
        fs::write(&file, b"x").unwrap();

        assert!(classify_inbox(&file, dir.path(), &preexisting).is_none());
    }

    #[test]
    fn inbox_event_outside_root_is_suppressed() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = other.path().join("report.md");
        fs::write(&file, b"x").unwrap();

        assert!(classify_inbox(&file, dir.path(), &preexisting).is_none());
    }

    #[test]
    fn inbox_event_for_binary_file_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("dump.bin");
        fs::write(&file, [0u8, 159, 146, 150]).unwrap();

        assert!(
            classify_inbox(&file, dir.path(), &preexisting).is_none(),
            "non-text files must not auto-open"
        );
    }

    #[test]
    fn inbox_event_for_large_file_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("huge.log");
        // Above the normal-open threshold the file would open in large-file mode
        // (syntax and rendered preview disabled), which defeats the inbox's
        // render-on-arrival purpose, so it must not auto-open.
        let big = vec![b'a'; (writ_core::file_ops::THRESHOLD_NORMAL_BYTES + 1) as usize];
        fs::write(&file, &big).unwrap();

        assert!(
            classify_inbox(&file, dir.path(), &preexisting).is_none(),
            "files above the normal-open threshold must not auto-open into the inbox"
        );
    }

    #[test]
    fn inbox_event_for_directory_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let sub = dir.path().join("new-dir");
        fs::create_dir(&sub).unwrap();

        assert!(classify_inbox(&sub, dir.path(), &preexisting).is_none());
    }

    #[test]
    fn inbox_event_for_a_save_writ_just_made_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("agent-output.md");
        let bytes = b"# edited in writ";
        fs::write(&file, bytes).unwrap();

        // Saving a buffer writes the file it was opened from. Without the
        // stamp the write reads as a new arrival, which reopens the tab and
        // pulls the window forward on every keystroke burst.
        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(source_stamp_key(&file), bytes, now);
        }

        assert!(classify_inbox_event(
            &file,
            dir.path(),
            &preexisting,
            &set,
            DEFAULT_IGNORE_TTL,
            now
        )
        .is_none());
    }

    #[test]
    fn inbox_event_for_another_program_writing_the_same_file_surfaces() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("agent-output.md");

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(source_stamp_key(&file), b"# edited in writ", now);
        }
        fs::write(&file, b"# rewritten by the agent").unwrap();

        assert!(
            classify_inbox_event(
                &file,
                dir.path(),
                &preexisting,
                &set,
                DEFAULT_IGNORE_TTL,
                now
            )
            .is_some(),
            "the stamp fingerprints bytes, so it must not swallow a real arrival"
        );
    }

    #[test]
    fn inbox_event_for_deleted_path_is_suppressed() {
        let dir = tempdir().unwrap();
        let preexisting = std::collections::HashSet::new();
        let file = dir.path().join("gone.md");

        assert!(classify_inbox(&file, dir.path(), &preexisting).is_none());
    }
}
