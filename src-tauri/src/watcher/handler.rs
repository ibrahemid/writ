use crate::poison::recover_poison;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
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

}
