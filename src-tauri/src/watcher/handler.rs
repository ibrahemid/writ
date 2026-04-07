use crate::events::{emit_event, WritFrontendEvent};
use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebounceEventResult};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;
use tauri::AppHandle;
use tracing::{error, info};

pub type IgnoreSet = Arc<Mutex<HashSet<String>>>;

pub fn create_ignore_set() -> IgnoreSet {
    Arc::new(Mutex::new(HashSet::new()))
}

pub fn start_file_watcher(
    app: AppHandle,
    config_path: PathBuf,
    buffers_dir: PathBuf,
    ignore_set: IgnoreSet,
) -> Result<(), Box<dyn std::error::Error>> {
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

    std::mem::forget(debouncer);

    let config_path_clone = config_path.clone();
    let buffers_dir_clone = buffers_dir.clone();

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

                            let is_internal = {
                                let mut set = ignore_set.lock().unwrap_or_else(|e| e.into_inner());
                                set.remove(&filename)
                            };

                            if is_internal {
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
    });

    Ok(())
}
