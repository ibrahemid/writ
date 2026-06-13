use crate::poison::recover_poison;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};
use tracing::{error, info};
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::ignore::{IgnoreStamps, SuppressDecision, DEFAULT_IGNORE_TTL};

pub type IgnoreSet = Arc<Mutex<IgnoreStamps>>;

pub fn create_ignore_set() -> IgnoreSet {
    Arc::new(Mutex::new(IgnoreStamps::new()))
}

/// Opaque owner of the file watcher's debouncer.
///
/// Held by `AppState` so the watcher lives as long as the application.
/// Dropping this handle drops the inner `Debouncer`, which closes the
/// event channel and causes the watcher thread to exit cleanly.
pub struct WatcherHandle {
    _debouncer: Debouncer<RecommendedWatcher>,
}

pub fn start_file_watcher(
    bus: Arc<EventBus>,
    config_path: PathBuf,
    buffers_dir: PathBuf,
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
    if buffers_dir.exists() {
        debouncer
            .watcher()
            .watch(&buffers_dir, RecursiveMode::NonRecursive)?;
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
                            &buffers_dir,
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
/// Writ never writes inside workspace directories through this path
/// (buffer saves land in the buffers dir; save-to-source refreshes are
/// idempotent listing reloads), so no self-write suppression is needed.
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
/// Writ never writes inside inbox directories (buffer saves land in the
/// buffers dir), so no self-write suppression is needed; a user pointing
/// the inbox at already-open files is deduplicated downstream by the
/// open path's canonical source-path lookup.
pub fn start_inbox_watcher(
    bus: Arc<EventBus>,
    root: PathBuf,
) -> Result<WatcherHandle, Box<dyn std::error::Error>> {
    let watch_start = SystemTime::now();
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let mut debouncer = new_debouncer(Duration::from_millis(500), tx)?;
    debouncer.watcher().watch(&root, RecursiveMode::Recursive)?;

    info!(root = %root.display(), "inbox watcher started");

    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        if let Some(domain_event) =
                            classify_inbox_event(&event.path, &root, watch_start)
                        {
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

/// Classifies an inbox file-system event into [`WritEvent::InboxFileArrived`],
/// or suppresses it.
///
/// Mechanism only: reads file metadata, then defers the auto-open decision
/// to `writ_core::inbox::qualifies_for_auto_open` (containment, ignore set,
/// created-after-watch-start) and `file_ops::classify_path`. Only files that
/// classify as [`FileOpenMode::Normal`] auto-open: large-file-mode and binary
/// (hex) buffers disable the rendered view the inbox exists to show. The
/// debouncer does not distinguish create from modify, so the creation-timestamp comparison
/// is the discriminator: a pre-existing file modified while watched carries
/// a creation time before `watch_start` and is suppressed. Filesystems
/// without birth time fall back to mtime (see ADR-018).
pub fn classify_inbox_event(
    path: &Path,
    root: &Path,
    watch_start: SystemTime,
) -> Option<WritEvent> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    let created = metadata.created().or_else(|_| metadata.modified()).ok()?;
    if !writ_core::inbox::qualifies_for_auto_open(root, path, created, watch_start) {
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

/// Classifies a single file-system event into a domain event, or
/// suppresses it. Pure aside from a single `fs::read` to fingerprint
/// the file against the ignore set; callers test it directly with a
/// tempdir.
pub fn classify_watch_event(
    path: &Path,
    config_path: &Path,
    buffers_dir: &Path,
    ignore_set: &IgnoreSet,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    if path == config_path {
        let filename = config_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();

        let current_bytes = std::fs::read(path).ok();
        let decision = {
            let mut set = recover_poison(ignore_set.lock(), "watcher::handler::config_event");
            set.decide(&filename, current_bytes.as_deref(), now, ttl)
        };

        if decision == SuppressDecision::Suppress {
            return None;
        }

        info!("config file changed");
        return Some(WritEvent::ConfigChanged {
            keys: vec!["*".to_string()],
        });
    }

    if !path.starts_with(buffers_dir) {
        return None;
    }

    let filename = path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();

    if filename.is_empty() {
        return None;
    }

    // write_atomic persists through a NamedTempFile (`.tmp*`) created in the
    // buffers dir, so every internal save emits a create+delete pair for that
    // temp path. Buffer content files are `<uuid>.txt` and never start with a
    // dot, so a dotfile here is always non-buffer noise. Without this guard
    // the temp delete is classified as a BufferExternal change, the frontend
    // reloads the buffer registry, and an open preview iframe is torn down and
    // recreated mid-edit — the macOS webview hard-freeze this fix targets.
    if filename.starts_with('.') {
        return None;
    }

    let current_bytes = std::fs::read(path).ok();

    let decision = {
        let mut set = recover_poison(ignore_set.lock(), "watcher::handler::event_loop");
        set.decide(&filename, current_bytes.as_deref(), now, ttl)
    };

    if decision == SuppressDecision::Suppress {
        return None;
    }

    let change = if path.exists() {
        ExternalChange::Modified
    } else {
        ExternalChange::Deleted
    };
    info!(file = %filename, ?change, "external buffer file change");
    Some(WritEvent::BufferExternal {
        buffer_id: filename,
        change,
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

    #[test]
    fn classifies_config_path_as_config_changed() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        fs::write(&cfg, b"x").unwrap();
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();

        let event = classify_watch_event(
            &cfg,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );

        assert!(matches!(event, Some(WritEvent::ConfigChanged { .. })));
    }

    #[test]
    fn classifies_modified_buffer_file_as_buffer_external_modified() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let buf = buffers.join("draft-1.txt");
        fs::write(&buf, b"hello").unwrap();

        let event = classify_watch_event(
            &buf,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );

        match event {
            Some(WritEvent::BufferExternal { buffer_id, change }) => {
                assert_eq!(buffer_id, "draft-1.txt");
                assert_eq!(change, ExternalChange::Modified);
            }
            other => panic!("expected BufferExternal::Modified, got {:?}", other),
        }
    }

    #[test]
    fn ignores_atomic_write_temp_files_in_buffers_dir() {
        // write_atomic's NamedTempFile (`.tmp*`) create/delete must never be
        // surfaced as an external buffer change; doing so triggered a
        // frontend reload that recreated an open preview iframe and froze the
        // macOS webview.
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let tmp = buffers.join(".tmpA1b2C3");
        fs::write(&tmp, b"partial").unwrap();

        let modified = classify_watch_event(
            &tmp,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );
        assert!(modified.is_none(), "temp create must not be a buffer event");

        fs::remove_file(&tmp).unwrap();
        let deleted = classify_watch_event(
            &tmp,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );
        assert!(deleted.is_none(), "temp delete must not be a buffer event");
    }

    #[test]
    fn classifies_deleted_buffer_file_as_buffer_external_deleted() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let buf = buffers.join("gone.txt");

        let event = classify_watch_event(
            &buf,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );

        match event {
            Some(WritEvent::BufferExternal { buffer_id, change }) => {
                assert_eq!(buffer_id, "gone.txt");
                assert_eq!(change, ExternalChange::Deleted);
            }
            other => panic!("expected BufferExternal::Deleted, got {:?}", other),
        }
    }

    #[test]
    fn suppresses_event_matching_recent_ignore_fingerprint() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let buf = buffers.join("self.txt");
        let bytes = b"matching-bytes";
        fs::write(&buf, bytes).unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record("self.txt".to_string(), bytes, now);
        }

        let event = classify_watch_event(
            &buf,
            &cfg,
            &buffers,
            &set,
            DEFAULT_IGNORE_TTL,
            now,
        );

        assert!(event.is_none(), "expected internal write to be suppressed");
    }

    #[test]
    fn suppresses_internal_config_write() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let bytes = b"theme = \"dark\"\n";
        fs::write(&cfg, bytes).unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record("config.toml".to_string(), bytes, now);
        }

        let event = classify_watch_event(&cfg, &cfg, &buffers, &set, DEFAULT_IGNORE_TTL, now);

        assert!(event.is_none(), "internal config write must be suppressed");
    }

    #[test]
    fn emits_external_config_change_when_bytes_differ() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record("config.toml".to_string(), b"theme = \"dark\"\n", now);
        }

        fs::write(&cfg, b"theme = \"light\"\n").unwrap();

        let event = classify_watch_event(&cfg, &cfg, &buffers, &set, DEFAULT_IGNORE_TTL, now);

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
    fn ignores_paths_outside_both_config_and_buffers_dir() {
        let dir = tempdir().unwrap();
        let cfg = dir.path().join("config.toml");
        let buffers = dir.path().join("buffers");
        fs::create_dir_all(&buffers).unwrap();
        let unrelated = dir.path().join("unrelated.log");
        fs::write(&unrelated, b"x").unwrap();

        let event = classify_watch_event(
            &unrelated,
            &cfg,
            &buffers,
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        );

        assert!(event.is_none());
    }

    #[test]
    fn inbox_event_for_file_created_after_watch_start_surfaces() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let file = dir.path().join("report.md");
        fs::write(&file, b"# done").unwrap();

        match classify_inbox_event(&file, dir.path(), watch_start) {
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

        let watch_start = SystemTime::now() + Duration::from_secs(60);
        fs::write(&file, b"after").unwrap();

        assert!(
            classify_inbox_event(&file, dir.path(), watch_start).is_none(),
            "a file created before watch start must never auto-open"
        );
    }

    #[test]
    fn inbox_event_under_ignored_dir_is_suppressed() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let nested = dir.path().join("node_modules").join("pkg");
        fs::create_dir_all(&nested).unwrap();
        let file = nested.join("readme.md");
        fs::write(&file, b"x").unwrap();

        assert!(classify_inbox_event(&file, dir.path(), watch_start).is_none());
    }

    #[test]
    fn inbox_event_outside_root_is_suppressed() {
        let dir = tempdir().unwrap();
        let other = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let file = other.path().join("report.md");
        fs::write(&file, b"x").unwrap();

        assert!(classify_inbox_event(&file, dir.path(), watch_start).is_none());
    }

    #[test]
    fn inbox_event_for_binary_file_is_suppressed() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let file = dir.path().join("dump.bin");
        fs::write(&file, [0u8, 159, 146, 150]).unwrap();

        assert!(
            classify_inbox_event(&file, dir.path(), watch_start).is_none(),
            "non-text files must not auto-open"
        );
    }

    #[test]
    fn inbox_event_for_large_file_is_suppressed() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let file = dir.path().join("huge.log");
        // Above the normal-open threshold the file would open in large-file mode
        // (syntax and rendered preview disabled), which defeats the inbox's
        // render-on-arrival purpose, so it must not auto-open.
        let big = vec![b'a'; (writ_core::file_ops::THRESHOLD_NORMAL_BYTES + 1) as usize];
        fs::write(&file, &big).unwrap();

        assert!(
            classify_inbox_event(&file, dir.path(), watch_start).is_none(),
            "files above the normal-open threshold must not auto-open into the inbox"
        );
    }

    #[test]
    fn inbox_event_for_directory_is_suppressed() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let sub = dir.path().join("new-dir");
        fs::create_dir(&sub).unwrap();

        assert!(classify_inbox_event(&sub, dir.path(), watch_start).is_none());
    }

    #[test]
    fn inbox_event_for_deleted_path_is_suppressed() {
        let dir = tempdir().unwrap();
        let watch_start = SystemTime::now() - Duration::from_secs(60);
        let file = dir.path().join("gone.md");

        assert!(classify_inbox_event(&file, dir.path(), watch_start).is_none());
    }
}
