//! Following the folder an open file lives in.
//!
//! Writ opens files from anywhere: a git repository, `~/Downloads`, a folder
//! a colleague shared. Those files are edited by other programs too, and a tab
//! that has not looked at its file since it was opened is a tab that will
//! overwrite somebody else's work on its next save. The notes folder already
//! has a watcher of its own ([`super::handler::start_notes_watcher`]); this is
//! the rest of it.
//!
//! Two rules shape the whole module.
//!
//! **The parent folder is watched, never the file.** An atomic replace —
//! which is how Writ, git, and every careful editor write — creates a new file
//! and renames it over the old one, so the inode a file-scoped watch is bound
//! to stops being the file. The watch survives the rename only if it is on the
//! folder (<https://docs.rs/notify/latest/notify/>).
//!
//! **A folder is watched once and released by the last tab in it.** Ten notes
//! from one repository cost one watch, and closing nine of them keeps it. The
//! count is the notes themselves rather than a number, so the same note opened
//! twice cannot leak a reference.
//!
//! Where the native backend cannot watch a folder at all — a network mount, a
//! FileProvider tree, a folder whose contents are not readable — that one
//! folder falls back to [`notify::PollWatcher`] with content comparison. The
//! choice is per folder: one unwatchable share does not put the rest of the
//! machine on a timer.

use crate::poison::recover_poison;
use notify::{Config as NotifyConfig, PollWatcher, RecommendedWatcher, RecursiveMode, Watcher};
use notify_debouncer_mini::{new_debouncer_opt, Config as DebounceConfig, DebounceEventResult};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::ignore::{SuppressDecision, DEFAULT_IGNORE_TTL};

use super::handler::{ignore_key_path, IgnoreSet};

/// The debounce window both backends coalesce into, matching every other
/// watcher in the app.
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(500);

/// How often the fallback backend looks, when it is the one being used.
///
/// Slower than the debounce window on purpose: polling is what a folder gets
/// when nothing better works, and the cost of a pass is a `stat` and a read of
/// every file in it.
pub const POLL_INTERVAL: Duration = Duration::from_secs(4);

/// Which backend is watching a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatcherKind {
    /// The platform's own: FSEvents, inotify, ReadDirectoryChangesW.
    Native,
    /// Timed passes comparing contents, for a folder the native backend
    /// refused.
    Poll,
}

/// What came of asking for a folder to be watched.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WatchOutcome {
    /// The folder is watched, by this backend.
    Watching(WatcherKind),
    /// Nothing was added: the notes watcher already covers this folder.
    AlreadyCovered,
    /// The note has no file to follow yet.
    NoFile,
    /// Neither backend would watch the folder. The tab still works; it just
    /// will not hear about a change until something asks the file directly.
    Unwatchable,
}

/// Registering and releasing folder watches on one backend.
///
/// This is the seam the fallback is chosen through, and the reason it exists
/// as a trait: whether the native backend refuses a folder depends on the
/// filesystem under it, which a test machine cannot be made to have. A test
/// injects a backend that refuses and asserts which one was selected, rather
/// than asserting that polling works.
pub trait DirWatcher: Send {
    /// Starts reporting changes to files directly inside `dir`.
    fn watch_dir(&mut self, dir: &Path) -> Result<(), String>;
    /// Stops reporting them.
    fn unwatch_dir(&mut self, dir: &Path) -> Result<(), String>;
}

/// A debouncer seen as a folder watcher.
///
/// Non-recursive: the folder holding an open file is the unit, and a recursive
/// watch on, say, a home directory or a repository with a `node_modules` in it
/// would report a flood nothing here can use.
struct DebouncedDirs<W: Watcher> {
    debouncer: notify_debouncer_mini::Debouncer<W>,
}

impl<W: Watcher> DirWatcher for DebouncedDirs<W>
where
    notify_debouncer_mini::Debouncer<W>: Send,
{
    fn watch_dir(&mut self, dir: &Path) -> Result<(), String> {
        self.debouncer
            .watcher()
            .watch(dir, RecursiveMode::NonRecursive)
            .map_err(|e| e.to_string())
    }

    fn unwatch_dir(&mut self, dir: &Path) -> Result<(), String> {
        self.debouncer
            .watcher()
            .unwatch(dir)
            .map_err(|e| e.to_string())
    }
}

/// One watched folder and the open notes whose files live in it.
#[derive(Debug)]
struct WatchedDir {
    kind: WatcherKind,
    /// Note id to the file it was opened from. Its length is the folder's
    /// reference count, which is why it is a map of notes rather than a
    /// number: the same note asking twice cannot count twice.
    notes: HashMap<String, PathBuf>,
}

/// Which folders are watched, which notes put them there, and by which
/// backend.
///
/// Every path stored here has been through [`ignore_key_path`], the same
/// resolution the ignore stamps use, so a path the watcher delivers and a path
/// a save recorded are the same string.
pub struct OpenFileRegistry {
    native: Box<dyn DirWatcher>,
    poll: Box<dyn DirWatcher>,
    notes_root: PathBuf,
    dirs: HashMap<PathBuf, WatchedDir>,
    /// Note id to the folder holding its file, so releasing a note does not
    /// need the path the open used.
    homes: HashMap<String, PathBuf>,
}

impl OpenFileRegistry {
    /// A registry over two backends, skipping anything already covered by the
    /// notes watcher rooted at `notes_root`.
    pub fn new(native: Box<dyn DirWatcher>, poll: Box<dyn DirWatcher>, notes_root: &Path) -> Self {
        Self {
            native,
            poll,
            notes_root: ignore_key_path(notes_root),
            dirs: HashMap::new(),
            homes: HashMap::new(),
        }
    }

    /// Points the registry at a new notes folder.
    ///
    /// Folders already watched are left alone: a folder that has become part
    /// of the notes tree is now reported by two watchers, which costs a
    /// duplicate event and never a missed one, and the tabs that put it there
    /// will release it as they close.
    pub fn set_notes_root(&mut self, notes_root: &Path) {
        self.notes_root = ignore_key_path(notes_root);
    }

    /// Watches the folder `file` lives in, on behalf of `note_id`.
    ///
    /// Asking again for a note already counted is a no-op, so the open path
    /// can call this without knowing whether the tab is new.
    pub fn watch_parent_of(&mut self, note_id: &str, file: &Path) -> WatchOutcome {
        let file = ignore_key_path(file);
        let Some(dir) = file.parent().map(Path::to_path_buf) else {
            return WatchOutcome::NoFile;
        };
        if dir.starts_with(&self.notes_root) {
            return WatchOutcome::AlreadyCovered;
        }

        if let Some(existing) = self.dirs.get_mut(&dir) {
            existing.notes.insert(note_id.to_string(), file);
            self.homes.insert(note_id.to_string(), dir);
            return WatchOutcome::Watching(existing.kind);
        }

        let kind = match self.native.watch_dir(&dir) {
            Ok(()) => WatcherKind::Native,
            Err(native_error) => match self.poll.watch_dir(&dir) {
                Ok(()) => {
                    info!(
                        dir = %dir.display(),
                        error = %native_error,
                        "folder cannot be watched natively; polling it instead"
                    );
                    WatcherKind::Poll
                }
                Err(poll_error) => {
                    warn!(
                        dir = %dir.display(),
                        native_error = %native_error,
                        poll_error = %poll_error,
                        "folder cannot be watched at all"
                    );
                    return WatchOutcome::Unwatchable;
                }
            },
        };

        let mut notes = HashMap::new();
        notes.insert(note_id.to_string(), file);
        self.dirs.insert(dir.clone(), WatchedDir { kind, notes });
        self.homes.insert(note_id.to_string(), dir);
        WatchOutcome::Watching(kind)
    }

    /// Releases whatever `note_id` was holding. The folder's watch goes when
    /// the last note in it does.
    pub fn unwatch_parent_of(&mut self, note_id: &str) {
        let Some(dir) = self.homes.remove(note_id) else {
            return;
        };
        let Some(watched) = self.dirs.get_mut(&dir) else {
            return;
        };
        watched.notes.remove(note_id);
        if !watched.notes.is_empty() {
            return;
        }
        let kind = watched.kind;
        self.dirs.remove(&dir);
        let backend: &mut Box<dyn DirWatcher> = match kind {
            WatcherKind::Native => &mut self.native,
            WatcherKind::Poll => &mut self.poll,
        };
        if let Err(e) = backend.unwatch_dir(&dir) {
            // The folder being gone is the ordinary way this fails, and the
            // registry has already forgotten it either way.
            info!(dir = %dir.display(), error = %e, "releasing a folder watch failed");
        }
    }

    /// Which backend is watching `dir`, if anything is.
    ///
    /// Per folder rather than per watcher: the fallback is chosen one folder
    /// at a time, so there is no single answer for the process.
    pub fn kind(&self, dir: &Path) -> Option<WatcherKind> {
        self.dirs.get(&ignore_key_path(dir)).map(|d| d.kind)
    }

    /// The note `path` is open as, if any.
    ///
    /// This is also the whole filter on the event stream: a folder reports
    /// every file in it, and only the files that are open notes get past here.
    /// The temp file beside every atomic write, an editor's swap file, a sync
    /// client's in-flight copy — none of them is an open note, so none of them
    /// reaches the ignore set or the event bus.
    pub fn note_at(&self, path: &Path) -> Option<String> {
        let path = ignore_key_path(path);
        let dir = path.parent()?;
        let watched = self.dirs.get(dir)?;
        watched
            .notes
            .iter()
            .find(|(_, file)| *file == &path)
            .map(|(id, _)| id.clone())
    }

    /// Every folder currently watched.
    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.dirs.keys().cloned().collect()
    }
}

/// Opaque owner of the open-file watcher.
///
/// Held by `AppState` so the watcher lives as long as the application.
/// Dropping it drops the registry, which drops both debouncers, which closes
/// the channel and ends the handler thread.
pub struct OpenFileWatcher {
    registry: Arc<Mutex<OpenFileRegistry>>,
}

impl OpenFileWatcher {
    /// The registry, for the open and close paths to add to and take from.
    pub fn registry(&self) -> &Arc<Mutex<OpenFileRegistry>> {
        &self.registry
    }
}

/// Starts watching folders on behalf of open files.
///
/// Both backends report into one channel, so a folder that fell back to
/// polling arrives on the same path as every other and nothing downstream
/// knows the difference.
pub fn start_open_file_watcher(
    bus: Arc<EventBus>,
    ignore_set: IgnoreSet,
    notes_root: &Path,
) -> Result<OpenFileWatcher, Box<dyn std::error::Error>> {
    let (tx, rx) = mpsc::channel::<DebounceEventResult>();

    let native: notify_debouncer_mini::Debouncer<RecommendedWatcher> = new_debouncer_opt(
        DebounceConfig::default().with_timeout(DEBOUNCE_WINDOW),
        tx.clone(),
    )?;
    // Content comparison, not timestamps: a network share and a sync provider
    // are exactly the places an mtime is unreliable, and they are the reason
    // this backend exists at all.
    let poll: notify_debouncer_mini::Debouncer<PollWatcher> = new_debouncer_opt(
        DebounceConfig::default()
            .with_timeout(DEBOUNCE_WINDOW)
            .with_notify_config(
                NotifyConfig::default()
                    .with_poll_interval(POLL_INTERVAL)
                    .with_compare_contents(true),
            ),
        tx,
    )?;

    let registry = Arc::new(Mutex::new(OpenFileRegistry::new(
        Box::new(DebouncedDirs { debouncer: native }),
        Box::new(DebouncedDirs { debouncer: poll }),
        notes_root,
    )));

    let thread_registry = registry.clone();
    std::thread::spawn(move || {
        while let Ok(result) = rx.recv() {
            let events = match result {
                Ok(events) => events,
                Err(e) => {
                    error!("open file watcher error: {:?}", e);
                    continue;
                }
            };
            // One event per note per delivered batch. The debouncer coalesces
            // a window into a batch, so this is the per-window cap: a folder
            // another program is churning through cannot cost more than one
            // message per open tab, however many times each file was written.
            let mut told: HashSet<String> = HashSet::new();
            for event in events {
                let note_id = {
                    let registry =
                        recover_poison(thread_registry.lock(), "watcher::open_files::note_at");
                    registry.note_at(&event.path)
                };
                let Some(note_id) = note_id else {
                    continue;
                };
                if !told.insert(note_id.clone()) {
                    continue;
                }
                if let Some(domain_event) = classify_open_file_event(
                    &event.path,
                    &note_id,
                    &ignore_set,
                    DEFAULT_IGNORE_TTL,
                    Instant::now(),
                ) {
                    bus.emit(domain_event);
                }
            }
        }
        info!("open file watcher thread exiting");
    });

    info!("open file watcher started");
    Ok(OpenFileWatcher { registry })
}

/// Classifies a change to an open note's file into a domain event, or
/// suppresses it.
///
/// A write Writ itself made is suppressed. The stamp is keyed under the source
/// namespace by the file's resolved path and fingerprints the bytes (W3), so a
/// save of this file cannot swallow somebody else's edit to it, and a save of
/// a file with the same name in another folder cannot swallow anything at all.
///
/// `disk_hash` is the digest the editor compares its document against
/// ([`writ_core::hash::comparison_digest_hex`]), taken from the same read the
/// fingerprint used. A file whose bytes are not on this machine is reported
/// without one and without being read: reading it would make the sync provider
/// fetch it (ADR-028 §5).
pub fn classify_open_file_event(
    path: &Path,
    note_id: &str,
    ignore_set: &IgnoreSet,
    ttl: Duration,
    now: Instant,
) -> Option<WritEvent> {
    let removed = !path.exists();
    if removed {
        return Some(WritEvent::BufferExternal {
            buffer_id: note_id.to_string(),
            path: path.to_string_lossy().into_owned(),
            change: ExternalChange::Deleted,
            new_path: None,
            disk_hash: None,
        });
    }
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return None;
    }

    let evicted = writ_core::notes::guard::is_not_downloaded(
        writ_storage::buffer_store::dataless_flags(path),
    );
    let current_bytes = if evicted {
        None
    } else {
        std::fs::read(path).ok()
    };

    let key = writ_core::watcher::ignore::source_key(&ignore_key_path(path));
    let decision = {
        let mut set = recover_poison(ignore_set.lock(), "watcher::open_files::classify");
        set.decide(&key, current_bytes.as_deref(), now, ttl)
    };
    if decision == SuppressDecision::Suppress {
        return None;
    }

    Some(WritEvent::BufferExternal {
        buffer_id: note_id.to_string(),
        path: path.to_string_lossy().into_owned(),
        change: ExternalChange::Modified,
        new_path: None,
        disk_hash: current_bytes
            .as_deref()
            .map(writ_core::hash::comparison_digest_hex),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// A backend that records what it was asked to do, and can be told to
    /// refuse — which is what a network mount or an unreadable folder does,
    /// and what no test machine can be made to do for real.
    #[derive(Default)]
    struct FakeBackend {
        refuse: bool,
        watched: Arc<Mutex<Vec<PathBuf>>>,
        released: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl FakeBackend {
        fn refusing() -> Self {
            Self {
                refuse: true,
                ..Self::default()
            }
        }
    }

    impl DirWatcher for FakeBackend {
        fn watch_dir(&mut self, dir: &Path) -> Result<(), String> {
            if self.refuse {
                return Err("no watcher for this filesystem".to_string());
            }
            self.watched.lock().unwrap().push(dir.to_path_buf());
            Ok(())
        }

        fn unwatch_dir(&mut self, dir: &Path) -> Result<(), String> {
            if self.refuse {
                return Err("no watcher for this filesystem".to_string());
            }
            self.released.lock().unwrap().push(dir.to_path_buf());
            Ok(())
        }
    }

    /// A registry over two recording backends, with the log of what each was
    /// asked to watch.
    #[allow(clippy::type_complexity)]
    fn registry_with(
        native_refuses: bool,
        poll_refuses: bool,
        notes_root: &Path,
    ) -> (
        OpenFileRegistry,
        Arc<Mutex<Vec<PathBuf>>>,
        Arc<Mutex<Vec<PathBuf>>>,
        Arc<Mutex<Vec<PathBuf>>>,
    ) {
        let native = if native_refuses {
            FakeBackend::refusing()
        } else {
            FakeBackend::default()
        };
        let poll = if poll_refuses {
            FakeBackend::refusing()
        } else {
            FakeBackend::default()
        };
        let native_log = native.watched.clone();
        let native_released = native.released.clone();
        let poll_log = poll.watched.clone();
        (
            OpenFileRegistry::new(Box::new(native), Box::new(poll), notes_root),
            native_log,
            poll_log,
            native_released,
        )
    }

    fn make_set() -> IgnoreSet {
        super::super::handler::create_ignore_set()
    }

    #[test]
    fn a_file_opened_from_outside_the_notes_folder_puts_a_watch_on_its_folder() {
        let notes = tempdir().unwrap();
        let elsewhere = tempdir().unwrap();
        let file = elsewhere.path().join("README.md");
        fs::write(&file, b"x").unwrap();

        let (mut registry, native_log, poll_log, _) = registry_with(false, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &file),
            WatchOutcome::Watching(WatcherKind::Native)
        );
        assert_eq!(native_log.lock().unwrap().len(), 1);
        assert!(poll_log.lock().unwrap().is_empty());
        assert_eq!(
            registry.kind(elsewhere.path()),
            Some(WatcherKind::Native),
            "the folder, not the file, is what gets watched"
        );
    }

    #[test]
    fn a_note_inside_the_notes_folder_adds_no_second_watch() {
        // The notes watcher already covers it. A second watcher over the same
        // tree would report every change twice and cost a second recursive
        // watch on the folder most likely to be large.
        let notes = tempdir().unwrap();
        let note = notes.path().join("today.md");
        fs::write(&note, b"x").unwrap();

        let (mut registry, native_log, poll_log, _) = registry_with(false, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &note),
            WatchOutcome::AlreadyCovered
        );
        assert!(registry.watched_dirs().is_empty());
        assert!(native_log.lock().unwrap().is_empty());
        assert!(poll_log.lock().unwrap().is_empty());
    }

    #[test]
    fn a_note_nested_inside_the_notes_folder_adds_no_second_watch() {
        let notes = tempdir().unwrap();
        let nested = notes.path().join("projects").join("writ");
        fs::create_dir_all(&nested).unwrap();
        let note = nested.join("plan.md");
        fs::write(&note, b"x").unwrap();

        let (mut registry, _, _, _) = registry_with(false, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &note),
            WatchOutcome::AlreadyCovered
        );
    }

    #[test]
    fn a_folder_the_native_backend_refuses_falls_back_to_polling_alone() {
        // Asserting the selection, not that polling works: whether the native
        // backend refuses depends on the filesystem, which CI cannot provide.
        let notes = tempdir().unwrap();
        let share = tempdir().unwrap();
        let file = share.path().join("shared.md");
        fs::write(&file, b"x").unwrap();

        let (mut registry, native_log, poll_log, _) = registry_with(true, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &file),
            WatchOutcome::Watching(WatcherKind::Poll)
        );
        assert_eq!(registry.kind(share.path()), Some(WatcherKind::Poll));
        assert!(native_log.lock().unwrap().is_empty());
        assert_eq!(poll_log.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_fallback_is_chosen_one_folder_at_a_time() {
        // One unwatchable share must not put the rest of the machine on a
        // timer, so the two folders end up on different backends.
        let notes = tempdir().unwrap();
        let ok_dir = tempdir().unwrap();
        let ok_file = ok_dir.path().join("local.md");
        fs::write(&ok_file, b"x").unwrap();

        let native = FakeBackend {
            refuse: false,
            ..FakeBackend::default()
        };
        let native_log = native.watched.clone();
        // A backend that refuses only the share, which is what a mixed machine
        // looks like.
        struct Selective {
            refuse_under: PathBuf,
            inner: FakeBackend,
        }
        impl DirWatcher for Selective {
            fn watch_dir(&mut self, dir: &Path) -> Result<(), String> {
                if dir.starts_with(&self.refuse_under) {
                    return Err("no watcher for this filesystem".to_string());
                }
                self.inner.watch_dir(dir)
            }
            fn unwatch_dir(&mut self, dir: &Path) -> Result<(), String> {
                self.inner.unwatch_dir(dir)
            }
        }

        let share = tempdir().unwrap();
        let share_file = share.path().join("shared.md");
        fs::write(&share_file, b"x").unwrap();

        let selective = Selective {
            refuse_under: ignore_key_path(share.path()),
            inner: native,
        };
        let poll = FakeBackend::default();
        let poll_log = poll.watched.clone();
        let mut registry = OpenFileRegistry::new(Box::new(selective), Box::new(poll), notes.path());

        registry.watch_parent_of("local", &ok_file);
        registry.watch_parent_of("shared", &share_file);

        assert_eq!(registry.kind(ok_dir.path()), Some(WatcherKind::Native));
        assert_eq!(registry.kind(share.path()), Some(WatcherKind::Poll));
        assert_eq!(native_log.lock().unwrap().len(), 1);
        assert_eq!(poll_log.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_folder_neither_backend_will_watch_is_reported_rather_than_recorded() {
        let notes = tempdir().unwrap();
        let gone = tempdir().unwrap();
        let file = gone.path().join("nowhere.md");
        fs::write(&file, b"x").unwrap();

        let (mut registry, _, _, _) = registry_with(true, true, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &file),
            WatchOutcome::Unwatchable
        );
        assert!(
            registry.watched_dirs().is_empty(),
            "a folder nothing is watching must not be recorded as watched"
        );
    }

    #[test]
    fn one_folder_is_watched_once_however_many_notes_are_open_in_it() {
        let notes = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let one = repo.path().join("one.md");
        let two = repo.path().join("two.md");
        fs::write(&one, b"x").unwrap();
        fs::write(&two, b"x").unwrap();

        let (mut registry, native_log, _, _) = registry_with(false, false, notes.path());

        registry.watch_parent_of("note-1", &one);
        registry.watch_parent_of("note-2", &two);

        assert_eq!(native_log.lock().unwrap().len(), 1);
        assert_eq!(registry.watched_dirs().len(), 1);
    }

    #[test]
    fn the_folder_is_released_by_the_last_tab_in_it_and_not_before() {
        let notes = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let one = repo.path().join("one.md");
        let two = repo.path().join("two.md");
        fs::write(&one, b"x").unwrap();
        fs::write(&two, b"x").unwrap();

        let (mut registry, _, _, released) = registry_with(false, false, notes.path());

        registry.watch_parent_of("note-1", &one);
        registry.watch_parent_of("note-2", &two);

        registry.unwatch_parent_of("note-1");
        assert_eq!(registry.watched_dirs().len(), 1, "one tab is still open");
        assert!(released.lock().unwrap().is_empty());

        registry.unwatch_parent_of("note-2");
        assert!(registry.watched_dirs().is_empty());
        assert_eq!(released.lock().unwrap().len(), 1);
    }

    #[test]
    fn the_same_note_asking_twice_cannot_leak_a_reference() {
        let notes = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let file = repo.path().join("one.md");
        fs::write(&file, b"x").unwrap();

        let (mut registry, _, _, released) = registry_with(false, false, notes.path());

        registry.watch_parent_of("note-1", &file);
        registry.watch_parent_of("note-1", &file);
        registry.unwatch_parent_of("note-1");

        assert!(
            registry.watched_dirs().is_empty(),
            "a folder one note asked for twice must go when that note closes"
        );
        assert_eq!(released.lock().unwrap().len(), 1);
    }

    #[test]
    fn releasing_a_note_that_was_never_watched_does_nothing() {
        let notes = tempdir().unwrap();
        let (mut registry, _, _, released) = registry_with(false, false, notes.path());

        registry.unwatch_parent_of("never-opened");

        assert!(released.lock().unwrap().is_empty());
    }

    #[test]
    fn only_a_file_that_is_an_open_note_is_recognised() {
        // The whole filter on the event stream. Everything else a folder
        // reports — the temp file beside every atomic write, an editor's swap
        // file, a sync client's in-flight copy — stops here.
        let notes = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let file = repo.path().join("one.md");
        let sibling = repo.path().join("one.md.tmp");
        fs::write(&file, b"x").unwrap();
        fs::write(&sibling, b"x").unwrap();

        let (mut registry, _, _, _) = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &file);

        assert_eq!(registry.note_at(&file).as_deref(), Some("note-1"));
        assert_eq!(registry.note_at(&sibling), None);
        assert_eq!(registry.note_at(&repo.path().join("other.md")), None);
    }

    #[test]
    fn a_file_with_the_same_name_in_another_folder_is_not_the_open_note() {
        let notes = tempdir().unwrap();
        let here = tempdir().unwrap();
        let there = tempdir().unwrap();
        let file = here.path().join("notes.md");
        let namesake = there.path().join("notes.md");
        fs::write(&file, b"x").unwrap();
        fs::write(&namesake, b"x").unwrap();

        let (mut registry, _, _, _) = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &file);

        assert_eq!(registry.note_at(&namesake), None);
    }

    #[test]
    fn another_program_rewriting_an_open_file_surfaces_with_what_it_now_holds() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        fs::write(&file, b"written by somebody else\n").unwrap();

        match classify_open_file_event(
            &file,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        ) {
            Some(WritEvent::BufferExternal {
                buffer_id,
                path,
                change,
                new_path,
                disk_hash,
            }) => {
                assert_eq!(buffer_id, "note-1");
                assert_eq!(path, file.to_string_lossy());
                assert_eq!(change, ExternalChange::Modified);
                assert_eq!(new_path, None);
                assert_eq!(
                    disk_hash,
                    Some(writ_core::hash::comparison_digest_hex(
                        b"written by somebody else\n"
                    ))
                );
            }
            other => panic!("expected BufferExternal, got {other:?}"),
        }
    }

    #[test]
    fn a_save_writ_just_made_never_comes_back_as_somebody_elses_edit() {
        // The failure this closes looks exactly like data loss from the
        // outside: every save would raise a change on the file it just wrote,
        // and the tab would offer to reload the text it is already showing.
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        let bytes = b"# written in writ\n";
        fs::write(&file, bytes).unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(
                writ_core::watcher::ignore::source_key(&ignore_key_path(&file)),
                bytes,
                now,
            );
        }

        assert!(
            classify_open_file_event(&file, "note-1", &set, DEFAULT_IGNORE_TTL, now).is_none(),
            "writ's own write must not arrive back as an external change"
        );
    }

    #[test]
    fn a_stamp_does_not_swallow_the_next_edit_by_another_program() {
        // The stamp fingerprints the bytes, so it stops covering the file the
        // moment the file stops holding those bytes.
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        fs::write(&file, b"# written in writ\n").unwrap();

        let set = make_set();
        let now = Instant::now();
        {
            let mut guard = set.lock().unwrap();
            guard.record(
                writ_core::watcher::ignore::source_key(&ignore_key_path(&file)),
                b"# written in writ\n",
                now,
            );
        }
        fs::write(&file, b"# rewritten by another editor\n").unwrap();

        assert!(
            classify_open_file_event(&file, "note-1", &set, DEFAULT_IGNORE_TTL, now).is_some(),
            "an edit landing right after writ's own save must still surface"
        );
    }

    #[test]
    fn a_file_that_has_gone_surfaces_as_deleted_with_no_digest() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("gone.md");

        match classify_open_file_event(
            &file,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
        ) {
            Some(WritEvent::BufferExternal {
                change, disk_hash, ..
            }) => {
                assert_eq!(change, ExternalChange::Deleted);
                assert_eq!(disk_hash, None);
            }
            other => panic!("expected a deletion, got {other:?}"),
        }
    }

    #[test]
    fn a_folder_appearing_where_a_note_was_is_not_a_change_to_the_note() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("subfolder");
        fs::create_dir(&sub).unwrap();

        assert!(classify_open_file_event(
            &sub,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now()
        )
        .is_none());
    }

    #[test]
    fn moving_the_notes_folder_leaves_the_folders_already_watched_alone() {
        let notes = tempdir().unwrap();
        let moved_to = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let file = repo.path().join("one.md");
        fs::write(&file, b"x").unwrap();

        let (mut registry, _, _, _) = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &file);
        registry.set_notes_root(moved_to.path());

        assert_eq!(registry.watched_dirs().len(), 1);
        assert_eq!(registry.note_at(&file).as_deref(), Some("note-1"));

        let inside_new_root = moved_to.path().join("today.md");
        fs::write(&inside_new_root, b"x").unwrap();
        assert_eq!(
            registry.watch_parent_of("note-2", &inside_new_root),
            WatchOutcome::AlreadyCovered
        );
    }
}
