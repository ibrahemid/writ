use crate::events::{emit_event, WritFrontendEvent};
use crate::poison::recover_poison;
use notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tracing::{error, info};
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
    app: AppHandle,
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

    let config_path_clone = config_path.clone();
    let buffers_dir_clone = buffers_dir.clone();

    // TODO(events-bus): migrate config:changed and buffer:external emission
    // to core::events::bus when the bridge proves out. See bus_bridge.rs.
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            match result {
                Ok(events) => {
                    for event in events {
                        let path = &event.path;
                        if *path == config_path_clone {
                            info!("config file changed");
                            emit_event(
                                &app,
                                WritFrontendEvent::ConfigChanged {
                                    keys: vec!["*".to_string()],
                                },
                            )
                            .ok();
                        } else if path.starts_with(&buffers_dir_clone) {
                            let filename = path
                                .file_name()
                                .map(|s| s.to_string_lossy().to_string())
                                .unwrap_or_default();

                            if filename.is_empty() {
                                continue;
                            }

                            let current_bytes = std::fs::read(path).ok();

                            let decision = {
                                let mut set = recover_poison(
                                    ignore_set.lock(),
                                    "watcher::handler::event_loop",
                                );
                                set.decide(
                                    &filename,
                                    current_bytes.as_deref(),
                                    Instant::now(),
                                    ttl,
                                )
                            };

                            if decision == SuppressDecision::Suppress {
                                continue;
                            }

                            let change = if path.exists() { "modified" } else { "deleted" };
                            info!(file = %filename, change = change, "external buffer file change");
                            emit_event(
                                &app,
                                WritFrontendEvent::BufferExternal {
                                    buffer_id: filename,
                                    change: change.to_string(),
                                },
                            )
                            .ok();
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
