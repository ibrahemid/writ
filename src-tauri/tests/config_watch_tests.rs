//! The config watcher against a real filesystem.
//!
//! Writ writes `config.toml` the way it writes a note: into a sibling temp
//! file renamed over the original. The rename gives the config a new inode, so
//! a watch bound to the file stops hearing it on backends that watch inodes.
//! These run the platform's own watcher over a real folder, because an
//! injected backend cannot prove that a rename over the config, and an edit
//! made after it, still reach Writ.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use writ_core::config::WritConfig;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::ignore::{config_key, DEFAULT_IGNORE_TTL};
use writ_storage::config_store::ConfigStore;
use writ_tauri_lib::security::resolve_for_containment;
use writ_tauri_lib::watcher::handler::{create_ignore_set, start_file_watcher, IgnoreSet};

/// Long enough for the 500 ms debounce plus the platform's own notification
/// latency, and for the ignore TTL to be nowhere near expiry.
const SETTLE: Duration = Duration::from_secs(3);

/// The path the ignore stamps key on. The handler's own `ignore_key_path` is
/// crate-private; this is the same resolution, reached through the function
/// it delegates to.
fn resolved(path: &Path) -> PathBuf {
    resolve_for_containment(path)
        .map(PathBuf::from)
        .unwrap_or_else(|| path.to_path_buf())
}

/// Writes `bytes` to `path` the way another editor does: into a sibling temp
/// file, then renamed over the target.
fn write_by_temp_and_rename(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("writ-test-tmp");
    std::fs::write(&temp, bytes).expect("write temp");
    std::fs::rename(&temp, path).expect("rename over target");
}

/// Writes `config` the way `commands::config::persist_config` does: the
/// serialized bytes are stamped under the config's resolved path before the
/// store writes them.
fn persist_like_the_app(store: &ConfigStore, ignore: &IgnoreSet, config: &WritConfig) {
    let contents = store.serialize(config).expect("serialize");
    ignore.lock().expect("ignore set").record(
        config_key(&resolved(store.path())),
        contents.as_bytes(),
        Instant::now(),
    );
    store.write_serialized(&contents).expect("config write");
}

/// Every `ConfigChanged` the bus carried within `SETTLE`.
fn collect_config_changes(rx: &mpsc::Receiver<WritEvent>) -> Vec<WritEvent> {
    let deadline = Instant::now() + SETTLE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event @ WritEvent::ConfigChanged { .. }) => seen.push(event),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}

fn bus_with_channel() -> (Arc<EventBus>, mpsc::Receiver<WritEvent>) {
    let bus = Arc::new(EventBus::new());
    let (tx, rx) = mpsc::channel();
    bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });
    (bus, rx)
}

/// The round trip for a watcher handed `config_path`: a seeded config, Writ's
/// own stamped write absorbed, and another program's edit after it reported
/// exactly once.
fn assert_own_write_absorbed_and_later_edit_reported(config_path: &Path) {
    std::fs::write(config_path, b"[editor]\nfont_size = 14\n").expect("seed config");

    let ignore = create_ignore_set();
    let (bus, rx) = bus_with_channel();
    let _watcher = start_file_watcher(bus, config_path.to_path_buf(), ignore.clone())
        .expect("start the config watcher");
    // A late delivery of the seed write is the platform's business; the
    // assertions start from a quiet folder.
    collect_config_changes(&rx);

    let store = ConfigStore::new(config_path.to_path_buf());
    let mut config = WritConfig::default();
    config.editor.font_size = 20;
    persist_like_the_app(&store, &ignore, &config);
    let echoed = collect_config_changes(&rx);
    assert!(
        echoed.is_empty(),
        "Writ's own config write must not come back as an external change, saw {echoed:?}"
    );

    write_by_temp_and_rename(config_path, b"[editor]\nfont_size = 22\n");
    let seen = collect_config_changes(&rx);
    assert_eq!(
        seen.len(),
        1,
        "an edit after Writ's own write must reach the app exactly once, saw {seen:?}"
    );
    assert!(
        DEFAULT_IGNORE_TTL > SETTLE,
        "this test only means anything while the stamp is still live"
    );
}

#[test]
fn an_edit_after_writ_s_own_config_write_still_reaches_the_app() {
    let data = TempDir::new().expect("data dir");
    let folder = resolved(data.path());
    assert_own_write_absorbed_and_later_edit_reported(&folder.join("config.toml"));
}

#[cfg(unix)]
#[test]
fn a_config_reached_through_a_symlinked_folder_is_still_followed() {
    // The data folder Writ is handed can be a link, and the platform reports
    // events under the folder's real path.
    let root = TempDir::new().expect("root");
    let real = root.path().join("real");
    std::fs::create_dir(&real).expect("real folder");
    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&real, &linked).expect("link the folder");

    assert_own_write_absorbed_and_later_edit_reported(&linked.join("config.toml"));
}

#[test]
fn a_config_created_after_the_watcher_started_is_followed() {
    let data = TempDir::new().expect("data dir");
    let config_path = resolved(data.path()).join("config.toml");

    let (bus, rx) = bus_with_channel();
    let _watcher = start_file_watcher(bus, config_path.clone(), create_ignore_set())
        .expect("start the config watcher");
    collect_config_changes(&rx);

    write_by_temp_and_rename(&config_path, b"[editor]\nfont_size = 22\n");
    let seen = collect_config_changes(&rx);
    assert_eq!(
        seen.len(),
        1,
        "a config another program creates must reach the app exactly once, saw {seen:?}"
    );
}
