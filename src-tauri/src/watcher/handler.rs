use crate::poison::recover_poison;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info};
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::ignore::{IgnoreStamps, SuppressDecision, DEFAULT_IGNORE_TTL};

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
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = classify_watch_event(
                            &event.path,
                            &config_path,
                            &ignore_set,
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
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = classify_inbox_event(
                            &event.path,
                            &root,
                            &preexisting,
                            &ignore_set,
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
/// removed there.
///
/// This is the minimum needed to keep the index honest about a folder the user
/// also edits from Obsidian, a phone, or a sync client. Watching an open note
/// for a conflicting external edit is a separate job and is release 0.5.
///
/// Writ's own saves are stamped into `ignore_set` before they land, so they do
/// not arrive back here as somebody else's edit; the store indexes them itself.
pub fn start_notes_watcher(
    bus: Arc<EventBus>,
    root: PathBuf,
    ignore_set: IgnoreSet,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    info!(root = %root.display(), "notes watcher started");

    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) = classify_notes_event(
                            &event.path,
                            &root,
                            &ignore_set,
                            DEFAULT_IGNORE_TTL,
                            Instant::now(),
                        ) {
                            bus.emit(domain_event);
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
    if writ_core::workspace::path_has_ignored_component(root, path) {
        return None;
    }
    let name = path.file_name()?.to_string_lossy().into_owned();
    if writ_core::workspace::is_ignored_name(&name) {
        return None;
    }

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
