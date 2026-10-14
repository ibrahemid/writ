//! Polling a folder by size and modification time.
//!
//! What a folder gets when the native backend will not watch it: a network
//! mount, a FileProvider tree. `notify`'s own poller compares modification
//! times to the second, and asked to compare contents it reads every file in
//! the folder on every pass, which on a share is the whole folder over the
//! network every few seconds (TS-18). This one compares each entry's size and
//! modification time at full precision and never opens a file.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, SystemTime};

use notify::{Config, Event, EventHandler, EventKind, RecursiveMode, Watcher, WatcherKind};

use crate::poison::recover_poison;

/// What a pass remembers about one entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EntryStamp {
    /// Length in bytes.
    pub len: u64,
    /// Modification time, when the filesystem reports one.
    pub modified: Option<SystemTime>,
}

/// Every entry directly inside one folder, by path.
pub type Snapshot = HashMap<PathBuf, EntryStamp>;

/// What `dir` holds now, from metadata alone. A folder that cannot be listed
/// holds nothing, so every entry it had reads as gone.
pub fn snapshot(dir: &Path) -> Snapshot {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Snapshot::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            Some((
                entry.path(),
                EntryStamp {
                    len: metadata.len(),
                    modified: metadata.modified().ok(),
                },
            ))
        })
        .collect()
}

/// The entries that appeared, went, or changed size or modification time
/// between two passes, in path order.
pub fn changes(before: &Snapshot, after: &Snapshot) -> Vec<PathBuf> {
    let mut changed: Vec<PathBuf> = after
        .iter()
        .filter(|(path, stamp)| before.get(*path) != Some(*stamp))
        .map(|(path, _)| path.clone())
        .chain(
            before
                .keys()
                .filter(|path| !after.contains_key(*path))
                .cloned(),
        )
        .collect();
    changed.sort();
    changed
}

/// A [`Watcher`] that looks at each watched folder every interval.
pub struct MetadataPollWatcher {
    folders: Arc<Mutex<HashMap<PathBuf, Snapshot>>>,
    // Dropping the sender ends the polling thread at its next wake.
    _stop: mpsc::Sender<()>,
}

impl Watcher for MetadataPollWatcher {
    fn new<F: EventHandler>(mut event_handler: F, config: Config) -> notify::Result<Self> {
        let interval = config.poll_interval().unwrap_or(Duration::from_secs(4));
        let folders: Arc<Mutex<HashMap<PathBuf, Snapshot>>> = Arc::default();
        let (stop, stopped) = mpsc::channel::<()>();
        let polled = folders.clone();
        std::thread::Builder::new()
            .name("writ metadata poll".to_string())
            .spawn(move || {
                while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(interval) {
                    let watched: Vec<PathBuf> =
                        recover_poison(polled.lock(), "watcher::poll::pass")
                            .keys()
                            .cloned()
                            .collect();
                    for dir in watched {
                        let now = snapshot(&dir);
                        let changed = {
                            let mut folders = recover_poison(polled.lock(), "watcher::poll::pass");
                            // Unwatched while this pass was looking.
                            let Some(before) = folders.get_mut(&dir) else {
                                continue;
                            };
                            let changed = changes(before, &now);
                            *before = now;
                            changed
                        };
                        for path in changed {
                            event_handler
                                .handle_event(Ok(Event::new(EventKind::Any).add_path(path)));
                        }
                    }
                }
            })
            .map_err(notify::Error::io)?;
        Ok(Self {
            folders,
            _stop: stop,
        })
    }

    fn watch(&mut self, path: &Path, _recursive_mode: RecursiveMode) -> notify::Result<()> {
        if !path.is_dir() {
            return Err(notify::Error::path_not_found().add_path(path.to_path_buf()));
        }
        let first = snapshot(path);
        recover_poison(self.folders.lock(), "watcher::poll::watch")
            .insert(path.to_path_buf(), first);
        Ok(())
    }

    fn unwatch(&mut self, path: &Path) -> notify::Result<()> {
        match recover_poison(self.folders.lock(), "watcher::poll::unwatch").remove(path) {
            Some(_) => Ok(()),
            None => Err(notify::Error::watch_not_found().add_path(path.to_path_buf())),
        }
    }

    fn kind() -> WatcherKind {
        WatcherKind::PollWatcher
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(len: u64, secs: u64) -> EntryStamp {
        EntryStamp {
            len,
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
        }
    }

    #[test]
    fn a_change_in_size_alone_is_a_change() {
        let before = Snapshot::from([(PathBuf::from("/s/a.md"), stamp(4, 10))]);
        let after = Snapshot::from([(PathBuf::from("/s/a.md"), stamp(5, 10))]);
        assert_eq!(changes(&before, &after), vec![PathBuf::from("/s/a.md")]);
    }

    #[test]
    fn a_change_in_time_inside_one_second_is_a_change() {
        let mut later = stamp(4, 10);
        later.modified = later.modified.map(|t| t + Duration::from_millis(1));
        let before = Snapshot::from([(PathBuf::from("/s/a.md"), stamp(4, 10))]);
        let after = Snapshot::from([(PathBuf::from("/s/a.md"), later)]);
        assert_eq!(changes(&before, &after), vec![PathBuf::from("/s/a.md")]);
    }

    #[test]
    fn arrivals_and_departures_are_changes_and_the_rest_is_not() {
        let before = Snapshot::from([
            (PathBuf::from("/s/kept.md"), stamp(1, 1)),
            (PathBuf::from("/s/gone.md"), stamp(1, 1)),
        ]);
        let after = Snapshot::from([
            (PathBuf::from("/s/kept.md"), stamp(1, 1)),
            (PathBuf::from("/s/new.md"), stamp(1, 1)),
        ]);
        assert_eq!(
            changes(&before, &after),
            vec![PathBuf::from("/s/gone.md"), PathBuf::from("/s/new.md")]
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_pass_reads_metadata_and_never_opens_a_file() {
        // Unreadable, yet described: the snapshot is stat alone.
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("share.md");
        std::fs::write(&file, "on a share").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
        let seen = snapshot(dir.path());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(seen.get(&file).map(|s| s.len), Some(10));
    }

    #[test]
    fn a_watched_folder_reports_a_write_on_the_next_pass() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, rx) = mpsc::channel::<notify::Result<Event>>();
        let mut watcher = MetadataPollWatcher::new(
            tx,
            Config::default().with_poll_interval(Duration::from_millis(20)),
        )
        .unwrap();
        watcher
            .watch(dir.path(), RecursiveMode::NonRecursive)
            .unwrap();
        let file = dir.path().join("a.md");
        std::fs::write(&file, "x").unwrap();
        let event = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("a pass reports the write")
            .expect("no error");
        assert_eq!(event.paths, vec![file]);
        watcher.unwatch(dir.path()).unwrap();
        assert!(watcher.unwatch(dir.path()).is_err());
    }
}
