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
use notify_debouncer_mini::{
    new_debouncer_opt, Config as DebounceConfig, DebounceEventResult, DebouncedEvent,
};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::hash::Sha256Digest;
use writ_core::notes::guard::DiskState;
use writ_core::notes::identity::{
    classify_delete, classify_delete_by_content, DeleteVerdict, FileIdentity,
};
use writ_core::watcher::change_event::{modification_is_news, ExternalChange};
use writ_core::watcher::ignore::{SuppressDecision, DEFAULT_IGNORE_TTL};
use writ_core::watcher::pending::{hold_window, HeldRemoval, PendingRemovals};
use writ_core::watcher::sighting::{LastSeen, DEFAULT_SIGHTING_TTL};

use super::handler::{ignore_key_path, IgnoreSet};
use super::moves::{FileTracking, MoveOutcome, NoteFiles};

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

/// Who reports a folder's changes.
///
/// The registry records the folder of every open file, including folders it
/// does not watch itself. Which file belongs to which tab is a different
/// question from who is watching for changes to it, and the notes watcher
/// needs the first answer for the folders it owns the second half of. Keeping
/// only the folders this registry armed is what left a change inside the notes
/// folder with no way to find the tab holding that file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Coverage {
    /// This registry armed a backend for the folder and has to release it.
    Own(WatcherKind),
    /// Inside the notes tree, which `start_notes_watcher` already reports
    /// recursively. Recorded for the lookup and never armed: a second watch
    /// would report every change in it twice.
    NotesWatcher,
}

/// One folder and the open notes whose files live in it.
#[derive(Debug)]
struct WatchedDir {
    coverage: Coverage,
    /// Note id to the file it was opened from. Its length is the folder's
    /// reference count, which is why it is a map of notes rather than a
    /// number: the same note asking twice cannot count twice.
    notes: HashMap<String, PathBuf>,
}

/// Where every open file lives, which folders are watched, and by which
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

    /// Points the registry at a new notes folder, moving every folder that
    /// changed sides.
    ///
    /// Both directions have to be handled. A folder that has just become part
    /// of the notes tree must give up its own watch, or every change in it is
    /// reported twice. A folder that has just left it has been relying on the
    /// notes watcher, which no longer reaches it, so it needs a watch of its
    /// own — without this, moving the notes folder left every tab in the old
    /// one hearing nothing until it was closed and reopened.
    pub fn set_notes_root(&mut self, notes_root: &Path) {
        let root = ignore_key_path(notes_root);
        if root == self.notes_root {
            return;
        }
        self.notes_root = root;

        let moved_in: Vec<PathBuf> = self
            .dirs
            .iter()
            .filter(|(dir, watched)| {
                matches!(watched.coverage, Coverage::Own(_)) && dir.starts_with(&self.notes_root)
            })
            .map(|(dir, _)| dir.clone())
            .collect();
        for dir in moved_in {
            if let Some(watched) = self.dirs.get_mut(&dir) {
                let Coverage::Own(kind) = watched.coverage else {
                    continue;
                };
                watched.coverage = Coverage::NotesWatcher;
                self.release_backend(kind, &dir);
            }
        }

        let moved_out: Vec<PathBuf> = self
            .dirs
            .iter()
            .filter(|(dir, watched)| {
                watched.coverage == Coverage::NotesWatcher && !dir.starts_with(&self.notes_root)
            })
            .map(|(dir, _)| dir.clone())
            .collect();
        for dir in moved_out {
            match self.arm(&dir) {
                Some(kind) => {
                    if let Some(watched) = self.dirs.get_mut(&dir) {
                        watched.coverage = Coverage::Own(kind);
                    }
                }
                None => {
                    // Nothing will watch it. The notes are still open and
                    // still resolvable by path, they just get no events, which
                    // is what `Unwatchable` means everywhere else here.
                    self.forget_dir(&dir);
                }
            }
        }
    }

    /// Records the folder `file` lives in as `note_id`'s home, watching it if
    /// nothing else already does.
    ///
    /// Asking again for a note already counted is a no-op, so the open path
    /// can call this without knowing whether the tab is new. Asking with a
    /// different file releases the note's previous folder first: a note whose
    /// path moved used to stay counted in the folder it left, holding that
    /// watch open for the life of the process and still answering to its old
    /// path.
    pub fn watch_parent_of(&mut self, note_id: &str, file: &Path) -> WatchOutcome {
        let file = ignore_key_path(file);
        let Some(dir) = file.parent().map(Path::to_path_buf) else {
            return WatchOutcome::NoFile;
        };
        if self.homes.get(note_id).is_some_and(|home| home != &dir) {
            self.unwatch_parent_of(note_id);
        }

        if let Some(existing) = self.dirs.get_mut(&dir) {
            existing.notes.insert(note_id.to_string(), file);
            let coverage = existing.coverage;
            self.homes.insert(note_id.to_string(), dir);
            return match coverage {
                Coverage::Own(kind) => WatchOutcome::Watching(kind),
                Coverage::NotesWatcher => WatchOutcome::AlreadyCovered,
            };
        }

        let (coverage, outcome) = if dir.starts_with(&self.notes_root) {
            (Coverage::NotesWatcher, WatchOutcome::AlreadyCovered)
        } else {
            match self.arm(&dir) {
                Some(kind) => (Coverage::Own(kind), WatchOutcome::Watching(kind)),
                None => return WatchOutcome::Unwatchable,
            }
        };

        let mut notes = HashMap::new();
        notes.insert(note_id.to_string(), file);
        self.dirs
            .insert(dir.clone(), WatchedDir { coverage, notes });
        self.homes.insert(note_id.to_string(), dir);
        outcome
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
        self.forget_dir(&dir);
    }

    /// Takes `dir` on whichever backend will have it, native first.
    ///
    /// `None` when neither will: the tab still works, it just hears nothing
    /// until something asks the file directly.
    fn arm(&mut self, dir: &Path) -> Option<WatcherKind> {
        match self.native.watch_dir(dir) {
            Ok(()) => Some(WatcherKind::Native),
            Err(native_error) => match self.poll.watch_dir(dir) {
                Ok(()) => {
                    info!(
                        dir = %dir.display(),
                        error = %native_error,
                        "folder cannot be watched natively; polling it instead"
                    );
                    Some(WatcherKind::Poll)
                }
                Err(poll_error) => {
                    warn!(
                        dir = %dir.display(),
                        native_error = %native_error,
                        poll_error = %poll_error,
                        "folder cannot be watched at all"
                    );
                    None
                }
            },
        }
    }

    /// Drops `dir` from the registry, releasing its watch if it had one.
    fn forget_dir(&mut self, dir: &Path) {
        let Some(watched) = self.dirs.remove(dir) else {
            return;
        };
        if let Coverage::Own(kind) = watched.coverage {
            self.release_backend(kind, dir);
        }
    }

    /// Hands the release to the backend that took the folder.
    fn release_backend(&mut self, kind: WatcherKind, dir: &Path) {
        let backend: &mut Box<dyn DirWatcher> = match kind {
            WatcherKind::Native => &mut self.native,
            WatcherKind::Poll => &mut self.poll,
        };
        if let Err(e) = backend.unwatch_dir(dir) {
            // The folder being gone is the ordinary way this fails, and the
            // registry has already forgotten it either way.
            info!(dir = %dir.display(), error = %e, "releasing a folder watch failed");
        }
    }

    /// Which backend is watching `dir`, if this registry is watching it.
    ///
    /// Per folder rather than per watcher: the fallback is chosen one folder
    /// at a time, so there is no single answer for the process. `None` for a
    /// folder inside the notes tree, which is recorded here but watched by
    /// [`super::handler::start_notes_watcher`].
    pub fn kind(&self, dir: &Path) -> Option<WatcherKind> {
        match self.dirs.get(&ignore_key_path(dir))?.coverage {
            Coverage::Own(kind) => Some(kind),
            Coverage::NotesWatcher => None,
        }
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

    /// Every folder this registry has armed a backend for.
    ///
    /// Folders inside the notes tree are recorded but not armed, so they are
    /// not here; [`Self::note_at`] answers for them all the same.
    pub fn watched_dirs(&self) -> Vec<PathBuf> {
        self.dirs
            .iter()
            .filter(|(_, watched)| matches!(watched.coverage, Coverage::Own(_)))
            .map(|(dir, _)| dir.clone())
            .collect()
    }
}

/// Which note a path is open as.
///
/// The seam the notes watcher asks through. It keeps
/// [`super::handler::start_notes_watcher`] independent of the registry, and it
/// is how a test gives that watcher a fixed set of open notes without opening
/// any files.
pub trait OpenNotes: Send + Sync {
    /// The note `path` is open as, if any.
    fn note_at(&self, path: &Path) -> Option<String>;
}

impl OpenNotes for Arc<Mutex<OpenFileRegistry>> {
    fn note_at(&self, path: &Path) -> Option<String> {
        recover_poison(self.lock(), "watcher::open_files::OpenNotes::note_at").note_at(path)
    }
}

/// The answer when there is no registry to ask, which is what a failed
/// open-file watcher leaves behind. Nothing is open, so nothing is routed.
pub struct NoOpenNotes;

impl OpenNotes for NoOpenNotes {
    fn note_at(&self, _path: &Path) -> Option<String> {
        None
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

    /// The same registry seen as a lookup, for the notes watcher to route by.
    pub fn open_notes(&self) -> Arc<dyn OpenNotes> {
        Arc::new(self.registry.clone())
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
    tracking: FileTracking,
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
        // Outside the batch loop: the event a read of this watcher's own
        // raises on Linux arrives in a later batch than the change that
        // caused it, so a record scoped to one batch would never see it.
        let mut seen = LastSeen::new();
        // A removal waits for the delivery that might answer it, so the wait
        // for the next event ends at its deadline rather than whenever the
        // folder happens to change again.
        let pending = RefCell::new(PendingRemovals::new(hold_window(DEBOUNCE_WINDOW)));
        loop {
            // Read out before the match: the borrow would otherwise stand for
            // the whole of it, and the timeout arm needs the same cell.
            let due = pending.borrow().deadline();
            let result = match due {
                Some(due) => match rx.recv_timeout(due.saturating_duration_since(Instant::now())) {
                    Ok(result) => result,
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let now = Instant::now();
                        let answers =
                            answer_held_removals(&mut pending.borrow_mut(), &[], &tracking, now);
                        for (_, event) in answers {
                            bus.emit(event);
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
            let events = match result {
                Ok(events) => events,
                Err(e) => {
                    error!("open file watcher error: {:?}", e);
                    continue;
                }
            };
            for message in report_delivery(
                &events,
                &thread_registry,
                &ignore_set,
                &mut seen,
                &pending,
                &tracking,
                Instant::now(),
            ) {
                bus.emit(message);
            }
        }
        info!("open file watcher thread exiting");
    });

    info!("open file watcher started");
    Ok(OpenFileWatcher { registry })
}

/// Everything one delivery has to say to the tabs it names, in the order they
/// are told.
///
/// The removals this delivery answers come first, and each one's note goes into
/// the per-delivery `told` set before the delivery's own events are read. That
/// seeding is load-bearing rather than tidy: the delivery carrying the other
/// half of a rename names the old path as well as the new one, and the old path
/// on its own reads as a file that went. Without it one rename costs the tab a
/// move and a contradictory removal behind it.
///
/// `told` is also the per-window cap in its own right. The debouncer coalesces
/// a window into one delivery, so a folder another program is churning through
/// cannot cost more than one message per open tab however many times each file
/// in it was written.
fn report_delivery(
    events: &[DebouncedEvent],
    registry: &Mutex<OpenFileRegistry>,
    ignore_set: &IgnoreSet,
    seen: &mut LastSeen,
    pending: &RefCell<PendingRemovals>,
    tracking: &FileTracking,
    now: Instant,
) -> Vec<WritEvent> {
    let mut messages: Vec<WritEvent> = Vec::new();
    let mut told: HashSet<String> = HashSet::new();
    // A rename arrives as the old path leaving and the new one appearing in
    // the same window, so the batch is where a file that moved is found again.
    let batch: Vec<PathBuf> = events.iter().map(|event| event.path.clone()).collect();
    for (note_id, event) in answer_held_removals(&mut pending.borrow_mut(), &batch, tracking, now) {
        told.insert(note_id);
        messages.push(event);
    }
    for event in events {
        let note_id = {
            let registry = recover_poison(registry.lock(), "watcher::open_files::note_at");
            registry.note_at(&event.path)
        };
        let Some(note_id) = note_id else {
            continue;
        };
        if !told.insert(note_id.clone()) {
            continue;
        }
        if let Some(domain_event) = report_open_file_event(
            &event.path,
            &note_id,
            ignore_set,
            seen,
            DEFAULT_IGNORE_TTL,
            now,
            &VanishedContext {
                batch: &batch,
                tracking,
                hold: pending,
            },
        ) {
            messages.push(domain_event);
        }
    }
    messages
}

/// What one *delivered* event for an open note's file is worth telling its
/// tab, or nothing.
///
/// This, rather than [`classify_open_file_event`], is what the watcher thread
/// calls, and the two are separate so that the record of what has already been
/// looked at cannot be skipped by the caller that matters. An event describing
/// the file exactly as this watcher last found it is dropped before anything
/// opens it, which is what keeps a classification's own read from arriving
/// back as the next change on Linux ([`writ_core::watcher::sighting`]).
pub fn report_open_file_event(
    path: &Path,
    note_id: &str,
    ignore_set: &IgnoreSet,
    seen: &mut LastSeen,
    ttl: Duration,
    now: Instant,
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    if !seen.is_news(
        path,
        super::handler::look_at(path),
        now,
        DEFAULT_SIGHTING_TTL,
    ) {
        return None;
    }
    classify_open_file_event(path, note_id, ignore_set, ttl, now, vanished)
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
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    // A path holding a directory holds no note, the same as a path holding
    // nothing. Dropping the event instead left the tab on a dead id, saving
    // into `Is a directory` rather than saying the file is gone.
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return open_note_vanished(note_id, path, vanished);
    }

    let current_bytes = readable_bytes(path);

    let key = writ_core::watcher::ignore::source_key(&ignore_key_path(path));
    let decision = {
        let mut set = recover_poison(ignore_set.lock(), "watcher::open_files::classify");
        set.decide(&key, current_bytes.as_deref(), now, ttl)
    };
    if decision == SuppressDecision::Suppress {
        return None;
    }

    let came_back = vanished.tracking.files.note_file_returned(note_id, path);
    open_note_modification(note_id, path, current_bytes.as_deref(), came_back, vanished)
}

/// The modification event for a tab, when there is one to send.
///
/// A watcher reports what the filesystem told it, and a report can be about a
/// write the tab already read: FSEvents delivers on its own schedule, so the
/// write that seeded a file can arrive after Writ opened it, and on a loaded
/// runner it does. Handing that to the tab shows the user an external-change
/// notice for the bytes in front of them. Whether it is news is
/// [`modification_is_news`]'s call, on the digest Writ recorded when it last
/// read or wrote the file.
///
/// A tab with nothing on record gets the report: with no digest to compare
/// against, silence would be a claim about bytes nobody read.
fn open_note_modification(
    note_id: &str,
    path: &Path,
    bytes: Option<&[u8]>,
    came_back: bool,
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    let last_read = vanished
        .tracking
        .files
        .last_disk_state(note_id)
        .map(|state| state.hash);
    let on_disk = bytes.map(writ_core::hash::sha256_bytes);
    modification_is_news(last_read, on_disk, came_back)
        .then(|| open_note_modified(note_id, path, bytes))
}

/// The file's bytes, or `None` where reading them is the wrong thing to do.
///
/// A file whose bytes are not on this machine is reported without being read:
/// reading it would make the sync provider fetch it (ADR-028 §5).
fn readable_bytes(path: &Path) -> Option<Vec<u8>> {
    if writ_core::notes::guard::is_not_downloaded(writ_storage::buffer_store::dataless_flags(path))
    {
        return None;
    }
    std::fs::read(path).ok()
}

/// The event telling `note_id`'s tab its file now holds `bytes`.
///
/// Shared by both watchers so a tab cannot get a different payload depending
/// on which side of the notes folder its file happens to sit
/// ([`super::handler::route_notes_change_to_open_tab`]).
pub fn open_note_modified(note_id: &str, path: &Path, bytes: Option<&[u8]>) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: note_id.to_string(),
        path: path.to_string_lossy().into_owned(),
        change: ExternalChange::Modified,
        new_path: None,
        disk_hash: bytes.map(writ_core::hash::comparison_digest_hex),
    }
}

/// The event telling `note_id`'s tab its file is gone.
pub fn open_note_removed(note_id: &str, path: &Path) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: note_id.to_string(),
        path: path.to_string_lossy().into_owned(),
        change: ExternalChange::Removed,
        new_path: None,
        disk_hash: None,
    }
}

/// The event telling `note_id`'s tab its file is at `to` now.
///
/// `path` is where the file was, so the tab can recognise the message as
/// being about the file it is holding. The digest is the file's, read at its
/// new home: a move changes no bytes, so this is what the tab already has, and
/// carrying it is what lets the editor confirm that rather than assume it.
pub fn open_note_moved(note_id: &str, from: &Path, to: &Path) -> WritEvent {
    WritEvent::BufferExternal {
        buffer_id: note_id.to_string(),
        path: from.to_string_lossy().into_owned(),
        change: ExternalChange::Moved,
        new_path: Some(to.to_string_lossy().into_owned()),
        disk_hash: readable_bytes(to)
            .as_deref()
            .map(writ_core::hash::comparison_digest_hex),
    }
}

/// Everything needed to decide what a file leaving its path means.
pub struct VanishedContext<'a> {
    /// Every path the watcher delivered in this batch. A rename shows up as
    /// the old path leaving and the new one arriving together, so this is
    /// where a file that moved is found again.
    pub batch: &'a [PathBuf],
    /// The identity probe and the record of what each tab's file is.
    pub tracking: &'a FileTracking,
    /// Where a removal no delivery has answered yet waits for the one that
    /// might ([`writ_core::watcher::pending`]). A rename whose halves land in
    /// different windows is a deletion to the first of them, and this is what
    /// keeps that answer from being given before the second window has been.
    pub hold: &'a RefCell<PendingRemovals>,
}

/// Longest a folder listing may be when looking for a file that left it.
///
/// The batch is the first place a moved file is looked for and covers a rename
/// that arrives paired with its own deletion, which is the ordinary case. The
/// listing is the second, for a rename whose halves land in different windows,
/// and it costs one metadata read per file in the folder. A folder past this
/// many files is left to the batch alone rather than paying a full listing for
/// each deletion in it.
const MAX_FOLDER_CANDIDATES: usize = 4096;

/// The paths that could be holding the file that left, and where each came
/// from.
///
/// The two are kept apart because they cost differently. An id is read from
/// every candidate, which is a `stat`. Bytes are read only from the batch:
/// hashing the folder a note left would read every note in it, and on a share
/// that is one deletion pulling four thousand files over the network.
pub struct Candidates {
    /// Paths this watcher's own window named. A rename arrives as the old
    /// path leaving and the new one appearing together, so this is where a
    /// file that moved is ordinarily found.
    pub batch: Vec<PathBuf>,
    /// The rest of the folder the file left, for a rename whose halves land in
    /// different windows.
    pub folder: Vec<PathBuf>,
}

impl Candidates {
    /// Every candidate in the order they are considered: the batch, then the
    /// folder.
    pub fn all(&self) -> impl Iterator<Item = &PathBuf> {
        self.batch.iter().chain(self.folder.iter())
    }
}

/// The paths that could be holding the file that left `path`.
///
/// The batch first, then the folder the file left, and the file's own path is
/// never a candidate for itself.
///
/// Within each of those the candidates are sorted lexically, so the same set
/// of names answers the same way on any volume rather than in `read_dir`
/// order. Order decides the answer where the same file is honestly at more
/// than one path, because a hard link is one file with two names. A path under
/// `notes_root` sorts ahead of the rest, since that is the one Writ keeps a
/// note in; the test is textual, so where the folder watch and the notes root
/// name one directory differently it does not fire and lexical order alone
/// decides. Past `MAX_FOLDER_CANDIDATES` which names are in the set is the
/// listing's answer.
///
/// Every path this produces is one a watcher covers: the batch is one
/// watcher's own window, and the folder is the one the file left, which is
/// watched because a tab's file was in it. That invariant is what makes a
/// match here safe to follow — the tab lands somewhere its changes still reach
/// it. A candidate source that broke it would have to be checked against the
/// watched folders before it could be believed (ADR-033 §12).
pub fn candidates_for(path: &Path, batch: &[PathBuf], notes_root: Option<&Path>) -> Candidates {
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut collect = |paths: &mut dyn Iterator<Item = PathBuf>| {
        let mut out: Vec<PathBuf> = Vec::new();
        for candidate in paths {
            if candidate == path || !seen.insert(candidate.clone()) {
                continue;
            }
            out.push(candidate);
        }
        out.sort_by(|left, right| {
            let outside =
                |candidate: &PathBuf| !notes_root.is_some_and(|root| candidate.starts_with(root));
            outside(left)
                .cmp(&outside(right))
                .then_with(|| left.cmp(right))
        });
        out
    };
    let named = collect(&mut batch.iter().cloned());
    let folder = collect(
        &mut path
            .parent()
            .and_then(|dir| std::fs::read_dir(dir).ok())
            .into_iter()
            .flat_map(|entries| entries.flatten().take(MAX_FOLDER_CANDIDATES))
            .map(|entry| entry.path()),
    );
    Candidates {
        batch: named,
        folder,
    }
}

/// What `note_id`'s tab is told when its file is no longer at `path`.
///
/// `None` when the tab has already been told — two watchers can see one file
/// leave one folder, and the record is what makes the second one silent.
///
/// A move is applied before it is announced. The row is where the next save
/// reads its destination, so a tab told about a move it has not been given yet
/// would write to the old path in the window between the two.
pub fn open_note_vanished(
    note_id: &str,
    path: &Path,
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    let files = vanished.tracking.files.as_ref();
    let Some(before) = files.identity_of(note_id) else {
        // Nothing to compare against, so nothing can be claimed about where
        // the file went. That the path is empty is still true and still the
        // tab's business.
        return files
            .note_file_removed(note_id, path)
            .then(|| open_note_removed(note_id, path));
    };
    // An identity that cannot recognise its file anywhere else makes the
    // verdict a foregone conclusion, so nothing is probed for it. On the
    // volumes that produce one there is no id to read and a probe describes
    // the file instead, which means reading it: one deletion in a folder of
    // four thousand notes on a share would otherwise pull every one of them
    // over the network for an answer already known.
    let candidates = candidates_for(
        path,
        vanished.batch,
        vanished.tracking.files.notes_root().as_deref(),
    );
    let probed: Vec<(PathBuf, FileIdentity)> = if before.is_durable() {
        candidates
            .all()
            .filter_map(|candidate| {
                let identity = vanished.tracking.probe.identity_of(candidate)?;
                Some((candidate.clone(), identity))
            })
            .collect()
    } else {
        Vec::new()
    };

    match classify_delete(&before, &probed) {
        DeleteVerdict::Moved(to) => announce_move(files, note_id, path, &to),
        // Nothing carries the id, which is the answer both for a file that was
        // deleted and for one whose id a write nobody reported retired. The
        // bytes separate them.
        DeleteVerdict::Removed => {
            match same_bytes_in_the_batch(note_id, &candidates.batch, vanished) {
                Some(to) => announce_move(files, note_id, path, &to),
                // Nothing this delivery named holds the file, which is not
                // yet an answer: the window that would carry the other half
                // of a rename may not have closed. The removal waits, and the
                // record is left alone until it is announced, so a tab is not
                // marked off a file that is about to turn up one folder away
                // ([`answer_held_removals`]). The clock starts here because
                // here is where the delivery is being read.
                None => {
                    let held = HeldRemoval {
                        note_id: note_id.to_string(),
                        path: path.to_path_buf(),
                        identity: Some(before),
                        last: files.last_disk_state(note_id),
                        batch: vanished.batch.to_vec(),
                    };
                    if vanished.hold.borrow_mut().hold(held, Instant::now()) {
                        return None;
                    }
                    files
                        .note_file_removed(note_id, path)
                        .then(|| open_note_removed(note_id, path))
                }
            }
        }
        // The volume cannot say whether the file moved or went, so neither is
        // claimed and the write guard governs the next save exactly as it did
        // before identity was read at all.
        DeleteVerdict::ExternalModification => Some(open_note_modified(
            note_id,
            path,
            readable_bytes(path).as_deref(),
        )),
    }
}

/// The message a move verdict becomes, once the record has been asked to
/// follow it.
///
/// A move that could not be applied is not silence. The tab still names a path
/// its file is not at, so the honest answer is the one a removal gives: the tab
/// keeps its text and stops writing, rather than saving over whatever turns up
/// at the old path later. Failing that way is the poisoned lock, the row that
/// could not be read, the rename the store refused and the destination no
/// string can spell — every one of which leaves the note where it was while
/// its file is somewhere else. Only a tab already on the destination has
/// nothing to hear, which is one move seen by both watchers.
fn announce_move(
    files: &dyn NoteFiles,
    note_id: &str,
    from: &Path,
    to: &Path,
) -> Option<WritEvent> {
    match files.note_file_moved(note_id, from, to) {
        MoveOutcome::Followed => Some(open_note_moved(note_id, from, to)),
        MoveOutcome::AlreadyThere => None,
        MoveOutcome::Failed => files
            .note_file_removed(note_id, from)
            .then(|| open_note_removed(note_id, from)),
    }
}

/// The path in the batch holding the bytes `note_id` last read from its file,
/// if there is one.
///
/// The second way a vanished file is recognised, once a write nobody reported
/// has retired the id Writ holds for it
/// ([`writ_core::notes::identity::classify_delete_by_content`]). Only the
/// batch is read, never the folder listing, and only where the length already
/// matches: a deletion in a folder of four thousand notes costs the reads its
/// own window named and no more. A file whose bytes are not on this machine is
/// left unread, so a candidate on a sync provider is not fetched to answer
/// this (ADR-028 §5).
fn same_bytes_in_the_batch(
    note_id: &str,
    batch: &[PathBuf],
    vanished: &VanishedContext<'_>,
) -> Option<PathBuf> {
    let last = vanished.tracking.files.last_disk_state(note_id)?;
    match classify_delete_by_content(&last.hash, &digests_of(batch, &last)) {
        DeleteVerdict::Moved(to) => Some(to),
        DeleteVerdict::Removed | DeleteVerdict::ExternalModification => None,
    }
}

/// What each candidate that could be holding `last`'s bytes holds now.
///
/// Only where the length already matches, so a deletion in a folder of four
/// thousand notes costs the reads its own window named and no more.
fn digests_of(candidates: &[PathBuf], last: &DiskState) -> Vec<(PathBuf, Sha256Digest)> {
    candidates
        .iter()
        .filter(|candidate| {
            std::fs::metadata(candidate).is_ok_and(|m| m.is_file() && m.len() == last.size)
        })
        .filter_map(|candidate| {
            let bytes = readable_bytes(candidate)?;
            Some((candidate.clone(), writ_core::hash::sha256_bytes(&bytes)))
        })
        .collect()
}

/// What the removals this watcher is holding have to say, now that `batch` has
/// been delivered.
///
/// Each one is looked for once more: in the delivery it vanished in and the
/// one that just arrived, and in the folder it left as it stands now. A hit
/// resolves it to a move; a removal past its deadline is announced as the
/// deletion it looked like all along, and the record is marked at that point
/// rather than when the path first went empty.
///
/// Resolving runs before expiry, so a removal this delivery answers is a move
/// however long it waited for the answer. The note ids come back with the
/// events because an answer here is the batch's one message for that note: the
/// events that follow it must not send a second (`told`).
///
/// A batch of nothing is how the thread asks on a timeout, when no delivery
/// came at all — the folder listing can still answer, and the deadline is
/// still due.
pub fn answer_held_removals(
    pending: &mut PendingRemovals,
    batch: &[PathBuf],
    tracking: &FileTracking,
    now: Instant,
) -> Vec<(String, WritEvent)> {
    let mut answers: Vec<(String, WritEvent)> = Vec::new();
    for note_id in pending.note_ids() {
        let Some(held) = pending.held(&note_id) else {
            continue;
        };
        // A file back at the path it left never went anywhere. The delivery
        // that put it back carries its own event for the tab, and this one
        // stops waiting rather than announcing a deletion behind it.
        if std::fs::metadata(&held.path).is_ok_and(|m| m.is_file()) {
            pending.forget(&note_id);
            continue;
        }
        let path = held.path.clone();
        let durable = held.identity.as_ref().is_some_and(FileIdentity::is_durable);
        let last = held.last;
        let candidates = candidates_for(
            &path,
            &held.candidates(batch),
            tracking.files.notes_root().as_deref(),
        );
        let probed: Vec<(PathBuf, FileIdentity)> = if durable {
            candidates
                .all()
                .filter_map(|candidate| {
                    let identity = tracking.probe.identity_of(candidate)?;
                    Some((candidate.clone(), identity))
                })
                .collect()
        } else {
            Vec::new()
        };
        // Ids from every candidate, bytes from the deliveries alone: hashing
        // the folder a note left would read every note in it, and a match on
        // content there would land the tab on any note holding the same text.
        let digests = match &last {
            Some(last) => digests_of(&candidates.batch, last),
            None => Vec::new(),
        };
        if let Some(to) = pending.resolve(&note_id, &probed, &digests) {
            // The hold is forgotten by `resolve`, so a move that could not be
            // applied falls through to the removal here and not to a second
            // wait for a delivery that has already answered.
            if let Some(event) = announce_move(tracking.files.as_ref(), &note_id, &path, &to) {
                answers.push((note_id.clone(), event));
            }
        }
    }
    for held in pending.expired(now) {
        if tracking.files.note_file_removed(&held.note_id, &held.path) {
            answers.push((
                held.note_id.clone(),
                open_note_removed(&held.note_id, &held.path),
            ));
        }
    }
    answers
}

/// The event a change the notes watcher already classified becomes for the tab
/// holding that file.
///
/// The notes watcher has done the filtering by this point — ignored names,
/// other clients' folders, and Writ's own stamped writes are all gone — so
/// this reads the file for its digest and builds the event, without asking the
/// ignore set a second question it has already answered.
pub fn open_note_change(
    note_id: &str,
    path: &Path,
    removed: bool,
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    if removed {
        return open_note_vanished(note_id, path, vanished);
    }
    let came_back = vanished.tracking.files.note_file_returned(note_id, path);
    let bytes = readable_bytes(path);
    open_note_modification(note_id, path, bytes.as_deref(), came_back, vanished)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A watcher holding removals for the window the running one holds them
    /// for, so a test sees the same wait production does.
    fn holding() -> RefCell<PendingRemovals> {
        RefCell::new(PendingRemovals::new(hold_window(DEBOUNCE_WINDOW)))
    }

    /// What a held removal becomes once its window passes with no delivery
    /// having answered it, which is what the watcher thread's own timeout
    /// does.
    fn announced_after_the_wait(
        pending: &RefCell<PendingRemovals>,
        tracking: &FileTracking,
    ) -> Vec<WritEvent> {
        let deadline = Instant::now() + hold_window(DEBOUNCE_WINDOW);
        answer_held_removals(&mut pending.borrow_mut(), &[], tracking, deadline)
            .into_iter()
            .map(|(_, event)| event)
            .collect()
    }
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

    /// A registry over two recording backends, with what each was asked to
    /// watch and to release.
    ///
    /// Both backends' release logs are here on purpose: asserting only the
    /// native one is how the release path for a polled folder went untested
    /// while reading as if it were covered.
    struct Harness {
        registry: OpenFileRegistry,
        native_watched: Arc<Mutex<Vec<PathBuf>>>,
        native_released: Arc<Mutex<Vec<PathBuf>>>,
        poll_watched: Arc<Mutex<Vec<PathBuf>>>,
        poll_released: Arc<Mutex<Vec<PathBuf>>>,
    }

    fn registry_with(native_refuses: bool, poll_refuses: bool, notes_root: &Path) -> Harness {
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
        let native_watched = native.watched.clone();
        let native_released = native.released.clone();
        let poll_watched = poll.watched.clone();
        let poll_released = poll.released.clone();
        Harness {
            registry: OpenFileRegistry::new(Box::new(native), Box::new(poll), notes_root),
            native_watched,
            native_released,
            poll_watched,
            poll_released,
        }
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

        let Harness {
            mut registry,
            native_watched: native_log,
            poll_watched: poll_log,
            ..
        } = registry_with(false, false, notes.path());

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

        let Harness {
            mut registry,
            native_watched: native_log,
            poll_watched: poll_log,
            ..
        } = registry_with(false, false, notes.path());

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

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());

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

        let Harness {
            mut registry,
            native_watched: native_log,
            poll_watched: poll_log,
            ..
        } = registry_with(true, false, notes.path());

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

        let Harness { mut registry, .. } = registry_with(true, true, notes.path());

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

        let Harness {
            mut registry,
            native_watched: native_log,
            ..
        } = registry_with(false, false, notes.path());

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

        let Harness {
            mut registry,
            native_released: released,
            ..
        } = registry_with(false, false, notes.path());

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

        let Harness {
            mut registry,
            native_released: released,
            ..
        } = registry_with(false, false, notes.path());

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
    fn a_polled_folder_is_released_on_the_backend_that_took_it() {
        // The release has to reach the backend holding the folder. Asserting
        // only the native log left this half reading as covered while nothing
        // checked it, and a poller nobody stops keeps reading every file in a
        // folder no tab is open on for the life of the process.
        let notes = tempdir().unwrap();
        let share = tempdir().unwrap();
        let file = share.path().join("shared.md");
        fs::write(&file, b"x").unwrap();

        let Harness {
            mut registry,
            native_released,
            poll_released,
            ..
        } = registry_with(true, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &file),
            WatchOutcome::Watching(WatcherKind::Poll)
        );
        registry.unwatch_parent_of("note-1");

        assert_eq!(
            poll_released.lock().unwrap().len(),
            1,
            "the poller must be told to stop"
        );
        assert!(
            native_released.lock().unwrap().is_empty(),
            "the native backend never had it"
        );
        assert!(registry.watched_dirs().is_empty());
    }

    #[test]
    fn a_note_whose_file_moves_to_another_folder_lets_the_old_one_go() {
        // A note's path changes when its file is renamed or moved. The old
        // folder used to keep the note counted in it, so its watch outlived
        // every tab in it, and a write to the file left behind still resolved
        // to a tab that had stopped editing it.
        let notes = tempdir().unwrap();
        let from = tempdir().unwrap();
        let to = tempdir().unwrap();
        let before = from.path().join("note.md");
        let after = to.path().join("note.md");
        fs::write(&before, b"x").unwrap();
        fs::write(&after, b"x").unwrap();

        let Harness {
            mut registry,
            native_released: released,
            ..
        } = registry_with(false, false, notes.path());

        registry.watch_parent_of("note-1", &before);
        registry.watch_parent_of("note-1", &after);

        assert_eq!(
            registry.watched_dirs(),
            vec![ignore_key_path(to.path())],
            "only the folder the file is in now is watched"
        );
        assert_eq!(
            released.lock().unwrap().len(),
            1,
            "the old folder was let go"
        );
        assert_eq!(registry.note_at(&after).as_deref(), Some("note-1"));
        assert_eq!(
            registry.note_at(&before),
            None,
            "the file it no longer edits must not resolve to it"
        );

        registry.unwatch_parent_of("note-1");
        assert!(registry.watched_dirs().is_empty());
    }

    #[test]
    fn releasing_a_note_that_was_never_watched_does_nothing() {
        let notes = tempdir().unwrap();
        let Harness {
            mut registry,
            native_released: released,
            ..
        } = registry_with(false, false, notes.path());

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

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
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

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &file);

        assert_eq!(registry.note_at(&namesake), None);
    }

    #[test]
    fn a_batch_that_names_another_file_says_nothing_about_an_open_one() {
        // What the watcher thread does with a delivered batch: every event is
        // answered for the path it names, and a note is reached only through
        // the path it is open at. A window in which one file was rewritten
        // names no other note in the folder, however many have tabs.
        let notes = tempdir().unwrap();
        let open = notes.path().join("open.md");
        let closed = notes.path().join("closed.md");
        fs::write(&open, b"x\n").unwrap();
        fs::write(&closed, b"x\n").unwrap();

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &open);
        let registry = Arc::new(Mutex::new(registry));

        let batch = [closed.clone(), notes.path().join("closed.md.writ-test-tmp")];
        let named: Vec<String> = batch
            .iter()
            .filter_map(|path| OpenNotes::note_at(&registry, path))
            .collect();
        assert!(
            named.is_empty(),
            "a batch naming only another file reaches no tab, named {named:?}"
        );
        assert_eq!(
            OpenNotes::note_at(&registry, &open).as_deref(),
            Some("note-1"),
            "the note itself is still reachable at its own path"
        );
    }

    /// Nothing recorded about any tab's file, and only the path itself in the
    /// batch: what the classifier is given when the question is about the
    /// change rather than about where a file went.
    fn nothing_tracked(path: &Path) -> (Vec<PathBuf>, FileTracking) {
        (vec![path.to_path_buf()], FileTracking::untracked())
    }

    /// A record of what the watcher decided, standing in for the state.
    #[derive(Default)]
    struct RecordingFiles {
        identity: Option<FileIdentity>,
        last: Option<writ_core::notes::guard::DiskState>,
        notes_root: Option<PathBuf>,
        moved: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
        removed: Arc<Mutex<Vec<PathBuf>>>,
        returned: Arc<Mutex<Vec<PathBuf>>>,
        already_told: Arc<Mutex<bool>>,
        /// What the record answers for a file that is at its path again: the
        /// tab was refusing to save to it, so hearing it is back is news
        /// whatever the file holds.
        was_removed: bool,
        /// What a move is answered with. `None` follows `already_told`, which
        /// is the ordinary shape: the first watcher applies the move and the
        /// second is told the row is already there.
        move_outcome: Option<MoveOutcome>,
    }

    impl NoteFiles for RecordingFiles {
        fn identity_of(&self, _note_id: &str) -> Option<FileIdentity> {
            self.identity.clone()
        }

        fn note_file_moved(&self, _note_id: &str, from: &Path, to: &Path) -> MoveOutcome {
            self.moved
                .lock()
                .unwrap()
                .push((from.to_path_buf(), to.to_path_buf()));
            self.move_outcome
                .unwrap_or(if *self.already_told.lock().unwrap() {
                    MoveOutcome::AlreadyThere
                } else {
                    MoveOutcome::Followed
                })
        }

        fn note_file_removed(&self, _note_id: &str, path: &Path) -> bool {
            self.removed.lock().unwrap().push(path.to_path_buf());
            !*self.already_told.lock().unwrap()
        }

        fn note_file_returned(&self, _note_id: &str, path: &Path) -> bool {
            self.returned.lock().unwrap().push(path.to_path_buf());
            self.was_removed
        }

        fn last_disk_state(&self, _note_id: &str) -> Option<writ_core::notes::guard::DiskState> {
            self.last
        }

        fn notes_root(&self) -> Option<PathBuf> {
            self.notes_root.clone()
        }
    }

    /// What Writ would have recorded after reading or writing `bytes`.
    fn last_read(bytes: &[u8]) -> writ_core::notes::guard::DiskState {
        writ_core::notes::guard::DiskState {
            hash: writ_core::hash::sha256_bytes(bytes),
            size: bytes.len() as u64,
            mtime: None,
        }
    }

    /// A probe that never answers, which is a volume with no file id on it.
    struct BlindProbe;

    impl writ_core::notes::identity::IdentityProbe for BlindProbe {
        fn identity_of(&self, _path: &Path) -> Option<FileIdentity> {
            None
        }
    }

    /// A probe that counts what it was asked, which is how the cost of a
    /// verdict is measured rather than assumed.
    #[derive(Default)]
    struct CountingProbe {
        asked: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl writ_core::notes::identity::IdentityProbe for CountingProbe {
        fn identity_of(&self, path: &Path) -> Option<FileIdentity> {
            self.asked.lock().unwrap().push(path.to_path_buf());
            crate::watcher::identity::read_identity(path)
        }
    }

    fn tracking_with(files: RecordingFiles) -> (FileTracking, Arc<RecordingFiles>) {
        let files = Arc::new(files);
        (
            FileTracking {
                probe: Arc::new(crate::watcher::identity::PlatformIdentity),
                files: files.clone(),
            },
            files,
        )
    }

    #[test]
    fn the_delivery_that_answers_a_removal_says_nothing_else_about_that_note() {
        // The delivery carrying the other half of a rename names the old path
        // as well as the new one, and the old path on its own reads as a file
        // that went. The removal is answered first and the note goes into the
        // per-delivery record, so the tab hears the move and nothing behind
        // it. Deleting that seeding costs one rename a move and a removal
        // contradicting it.
        let notes = tempdir().unwrap();
        let sub = notes.path().join("archive");
        fs::create_dir(&sub).unwrap();
        let before = notes.path().join("note.md");
        // A folder deeper than the listing the vanish delivery reads, so the
        // first delivery genuinely has no answer and the removal waits.
        let after = sub.join("moved-by-finder.md");
        fs::write(&before, b"text worth keeping").unwrap();
        let identity = crate::watcher::identity::read_identity(&before);
        fs::rename(&before, &after).unwrap();

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &before);
        let registry = Mutex::new(registry);

        let (tracking, _files) = tracking_with(RecordingFiles {
            identity,
            notes_root: Some(notes.path().to_path_buf()),
            ..RecordingFiles::default()
        });
        let ignore = make_set();
        let mut seen = LastSeen::new();
        let pending = holding();

        // The delivery the file left in: nothing here can answer it, so it
        // waits and the tab hears nothing.
        let vanish = [DebouncedEvent::new(
            before.clone(),
            notify_debouncer_mini::DebouncedEventKind::Any,
        )];
        let at = Instant::now();
        let first = report_delivery(
            &vanish, &registry, &ignore, &mut seen, &pending, &tracking, at,
        );
        assert!(first.is_empty(), "got {first:?}");

        // The delivery that answers it, naming both halves of the rename.
        let rename = [
            DebouncedEvent::new(
                after.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            ),
            DebouncedEvent::new(
                before.clone(),
                notify_debouncer_mini::DebouncedEventKind::Any,
            ),
        ];
        // Past the sighting record's lifetime on purpose. That record would
        // drop the second look at the old path on its own, and this is the
        // per-delivery rule rather than that one: the two guards are separate
        // and each has to hold without the other.
        let later = at + DEFAULT_SIGHTING_TTL + Duration::from_secs(1);
        let second = report_delivery(
            &rename, &registry, &ignore, &mut seen, &pending, &tracking, later,
        );
        match second.as_slice() {
            [WritEvent::BufferExternal {
                buffer_id,
                change,
                new_path,
                ..
            }] => {
                assert_eq!(buffer_id, "note-1");
                assert_eq!(change, &ExternalChange::Moved);
                assert_eq!(new_path.as_deref(), after.to_str());
            }
            other => panic!("expected one move and nothing else, got {other:?}"),
        }
    }

    #[test]
    fn a_move_the_record_could_not_apply_is_told_as_a_removal() {
        // The row could not follow the file: a poisoned lock, a row that would
        // not read, a rename the store refused. The file is somewhere the tab
        // does not name, and saying nothing leaves the next save writing to a
        // path its file left. What is true either way is that the path is
        // empty, so that is what the tab hears.
        let dir = tempdir().unwrap();
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        fs::write(&from, b"body").unwrap();
        let identity = crate::watcher::identity::read_identity(&from);
        fs::rename(&from, &to).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            move_outcome: Some(MoveOutcome::Failed),
            ..RecordingFiles::default()
        });
        let batch = vec![from.clone(), to.clone()];
        let event = open_note_vanished(
            "note-1",
            &from,
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        match event {
            Some(WritEvent::BufferExternal { change, .. }) => {
                assert_eq!(change, ExternalChange::Removed)
            }
            other => panic!("expected a removal, got {other:?}"),
        }
        assert_eq!(files.removed.lock().unwrap().as_slice(), &[from]);
    }

    #[test]
    fn a_move_the_tab_is_already_on_is_told_nothing() {
        // The second watcher to see one move. The row is where it belongs and
        // the tab heard the first watcher, so this costs no message and does
        // not mark the note off a file that is there.
        let dir = tempdir().unwrap();
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        fs::write(&from, b"body").unwrap();
        let identity = crate::watcher::identity::read_identity(&from);
        fs::rename(&from, &to).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            move_outcome: Some(MoveOutcome::AlreadyThere),
            ..RecordingFiles::default()
        });
        let batch = vec![from.clone(), to.clone()];
        let event = open_note_vanished(
            "note-1",
            &from,
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "got {event:?}");
        assert!(files.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_held_move_the_record_could_not_apply_is_told_as_a_removal() {
        // The same rule one delivery later, where the hold is answered rather
        // than the vanish. `resolve` has already forgotten the hold by then,
        // so a failure here has no second wait to fall back on and the removal
        // is announced in its place.
        let dir = tempdir().unwrap();
        let sub = dir.path().join("archive");
        fs::create_dir(&sub).unwrap();
        let before = dir.path().join("note.md");
        let after = sub.join("moved-by-finder.md");
        fs::write(&before, b"text worth keeping").unwrap();
        let identity = crate::watcher::identity::read_identity(&before);
        fs::rename(&before, &after).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            notes_root: Some(dir.path().to_path_buf()),
            move_outcome: Some(MoveOutcome::Failed),
            ..RecordingFiles::default()
        });
        let pending = holding();
        assert!(open_note_vanished(
            "note-1",
            &before,
            &VanishedContext {
                hold: &pending,
                batch: std::slice::from_ref(&before),
                tracking: &tracking,
            },
        )
        .is_none());

        let answers = answer_held_removals(
            &mut pending.borrow_mut(),
            std::slice::from_ref(&after),
            &tracking,
            Instant::now(),
        );
        match answers.as_slice() {
            [(note_id, WritEvent::BufferExternal { change, .. })] => {
                assert_eq!(note_id, "note-1");
                assert_eq!(change, &ExternalChange::Removed);
            }
            other => panic!("expected one removal, got {other:?}"),
        }
        assert_eq!(files.removed.lock().unwrap().as_slice(), &[before]);
        assert!(
            pending.borrow().is_empty(),
            "the hold was answered, so nothing may expire behind it"
        );
    }

    #[test]
    fn a_file_found_at_another_path_in_the_batch_is_a_move() {
        let dir = tempdir().unwrap();
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        fs::write(&from, b"body").unwrap();
        let identity = crate::watcher::identity::read_identity(&from);
        fs::rename(&from, &to).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            ..RecordingFiles::default()
        });
        let batch = vec![from.clone(), to.clone()];
        let event = open_note_vanished(
            "note-1",
            &from,
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        match event {
            Some(WritEvent::BufferExternal {
                buffer_id,
                path,
                change,
                new_path,
                disk_hash,
            }) => {
                assert_eq!(buffer_id, "note-1");
                assert_eq!(path, from.to_string_lossy());
                assert_eq!(change, ExternalChange::Moved);
                assert_eq!(new_path.as_deref(), Some(to.to_string_lossy().as_ref()));
                assert_eq!(
                    disk_hash,
                    Some(writ_core::hash::comparison_digest_hex(b"body")),
                    "a move changes no bytes, so the digest is the one the tab already holds"
                );
            }
            other => panic!("expected a move, got {other:?}"),
        }
        assert_eq!(
            files.moved.lock().unwrap().as_slice(),
            &[(from, to)],
            "the row has to move before the tab is told, or the next save writes to the old path"
        );
        assert!(files.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_file_found_nowhere_is_a_removal_even_in_a_folder_full_of_notes() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        fs::write(&gone, b"body").unwrap();
        // The other notes are written while the file is still there, so no
        // filesystem can hand one of them the inode number this one is using.
        // Which files a folder holds is the question; who allocates inodes is
        // not (the rule itself is covered in `identity`).
        for name in ["other.md", "third.md"] {
            fs::write(dir.path().join(name), b"still here").unwrap();
        }
        let identity = crate::watcher::identity::read_identity(&gone);
        fs::remove_file(&gone).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            ..RecordingFiles::default()
        });
        let batch = vec![gone.clone()];
        let pending = holding();
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &pending,
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "a removal waits before it is announced");
        assert!(pending.borrow().holds("note-1"));
        assert!(
            files.removed.lock().unwrap().is_empty(),
            "the tab is not marked off its file while the removal is waiting"
        );

        let announced = announced_after_the_wait(&pending, &tracking);
        match announced.as_slice() {
            [WritEvent::BufferExternal { change, .. }] => {
                assert_eq!(change, &ExternalChange::Removed);
            }
            other => panic!("expected a removal, got {other:?}"),
        }
        assert_eq!(files.removed.lock().unwrap().as_slice(), &[gone]);
        assert!(files.moved.lock().unwrap().is_empty());
    }

    #[test]
    fn a_rename_whose_halves_land_in_different_deliveries_is_still_a_move() {
        // What the watcher thread sees when a rename straddles a debounce
        // deadline: one delivery saying the path is empty, and the file
        // itself in the next. Answering the first on its own took the tab off
        // a file sitting one folder away.
        let dir = tempdir().unwrap();
        let sub = dir.path().join("archive");
        fs::create_dir(&sub).unwrap();
        let before = dir.path().join("note.md");
        let after = sub.join("moved-by-finder.md");
        fs::write(&before, b"text worth keeping").unwrap();
        let identity = crate::watcher::identity::read_identity(&before);
        fs::rename(&before, &after).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            notes_root: Some(dir.path().to_path_buf()),
            ..RecordingFiles::default()
        });
        let pending = holding();
        let first = open_note_vanished(
            "note-1",
            &before,
            &VanishedContext {
                hold: &pending,
                batch: std::slice::from_ref(&before),
                tracking: &tracking,
            },
        );
        assert!(first.is_none(), "nothing in that delivery could answer it");
        assert!(files.removed.lock().unwrap().is_empty());

        let second = vec![after.clone()];
        let answers = answer_held_removals(
            &mut pending.borrow_mut(),
            &second,
            &tracking,
            Instant::now(),
        );
        match answers.as_slice() {
            [(
                note_id,
                WritEvent::BufferExternal {
                    change, new_path, ..
                },
            )] => {
                assert_eq!(note_id, "note-1");
                assert_eq!(change, &ExternalChange::Moved);
                assert_eq!(new_path.as_deref(), after.to_str());
            }
            other => panic!("expected one move, got {other:?}"),
        }
        assert_eq!(
            files.moved.lock().unwrap().as_slice(),
            &[(before, after)],
            "and the row is moved before the tab hears about it"
        );
        assert!(files.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_rewrite_and_a_rename_in_different_deliveries_are_found_by_the_bytes() {
        // The same split, with the id on record retired by a rewrite nobody
        // reported. The bytes are what is left to recognise the file by, and
        // they are read from the deliveries rather than from the folder.
        let dir = tempdir().unwrap();
        let before = dir.path().join("note.md");
        let after = dir.path().join("renamed-by-finder.md");
        fs::write(&before, b"text worth keeping").unwrap();
        let retired = crate::watcher::identity::read_identity(&before);
        fs::remove_file(&before).unwrap();
        fs::write(&after, b"text worth keeping").unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"text worth keeping")),
            notes_root: Some(dir.path().to_path_buf()),
            ..RecordingFiles::default()
        });
        let pending = holding();
        let first = open_note_vanished(
            "note-1",
            &before,
            &VanishedContext {
                hold: &pending,
                batch: std::slice::from_ref(&before),
                tracking: &tracking,
            },
        );
        assert!(first.is_none());

        let second = vec![after.clone()];
        let answers = answer_held_removals(
            &mut pending.borrow_mut(),
            &second,
            &tracking,
            Instant::now(),
        );
        match answers.as_slice() {
            [(
                _,
                WritEvent::BufferExternal {
                    change, new_path, ..
                },
            )] => {
                assert_eq!(change, &ExternalChange::Moved);
                assert_eq!(new_path.as_deref(), after.to_str());
            }
            other => panic!("expected one move, got {other:?}"),
        }
        assert!(files.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_file_back_at_its_own_path_is_never_announced_as_a_deletion() {
        // A sync client landing an update is a delete and a create at one
        // path, and the two can arrive in different deliveries. The second one
        // carries its own event for the tab; the removal waiting behind it
        // stops waiting rather than following it with a deletion.
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, b"as writ left it").unwrap();
        let retired = crate::watcher::identity::read_identity(&path);
        fs::remove_file(&path).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"as writ left it")),
            notes_root: Some(dir.path().to_path_buf()),
            ..RecordingFiles::default()
        });
        let pending = holding();
        assert!(open_note_vanished(
            "note-1",
            &path,
            &VanishedContext {
                hold: &pending,
                batch: std::slice::from_ref(&path),
                tracking: &tracking,
            },
        )
        .is_none());

        fs::write(&path, b"as the sync client left it").unwrap();
        let answers = answer_held_removals(
            &mut pending.borrow_mut(),
            std::slice::from_ref(&path),
            &tracking,
            Instant::now() + hold_window(DEBOUNCE_WINDOW),
        );
        assert!(answers.is_empty(), "saw {answers:?}");
        assert!(pending.borrow().is_empty());
        assert!(files.removed.lock().unwrap().is_empty());
        assert!(files.moved.lock().unwrap().is_empty());
    }

    #[test]
    fn a_file_renamed_in_its_own_folder_is_found_without_the_batch() {
        // The halves of a rename can land in different windows, so the folder
        // the file left is looked at as well as the batch.
        let dir = tempdir().unwrap();
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        fs::write(&from, b"body").unwrap();
        let identity = crate::watcher::identity::read_identity(&from);
        fs::rename(&from, &to).unwrap();

        let (tracking, _files) = tracking_with(RecordingFiles {
            identity,
            ..RecordingFiles::default()
        });
        let event = open_note_vanished(
            "note-1",
            &from,
            &VanishedContext {
                hold: &holding(),
                batch: &[],
                tracking: &tracking,
            },
        );

        match event {
            Some(WritEvent::BufferExternal {
                change, new_path, ..
            }) => {
                assert_eq!(change, ExternalChange::Moved);
                assert_eq!(new_path.as_deref(), Some(to.to_string_lossy().as_ref()));
            }
            other => panic!("expected a move, got {other:?}"),
        }
    }

    #[test]
    fn an_identity_the_volume_will_not_give_degrades_to_a_modification() {
        // Selection, not correctness: what is asserted is that the fallback
        // was taken and the verdict stopped claiming anything, which is what
        // spec W4 asks for on a volume with no file id.
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");

        let (tracking, files) = tracking_with(RecordingFiles {
            identity: Some(FileIdentity::Fallback {
                path: gone.to_string_lossy().into_owned(),
                size: 4,
                mtime_ms: None,
                hash: writ_core::hash::sha256_bytes(b"body"),
            }),
            ..RecordingFiles::default()
        });
        let tracking = FileTracking {
            probe: Arc::new(BlindProbe),
            files: tracking.files.clone(),
        };
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &holding(),
                batch: &[],
                tracking: &tracking,
            },
        );

        match event {
            Some(WritEvent::BufferExternal {
                change, disk_hash, ..
            }) => {
                assert_eq!(change, ExternalChange::Modified);
                assert_eq!(disk_hash, None, "there is nothing at the path to hash");
            }
            other => panic!("expected a modification, got {other:?}"),
        }
        assert!(
            files.removed.lock().unwrap().is_empty(),
            "a tab must not stop writing on a verdict nothing could establish"
        );
        assert!(files.moved.lock().unwrap().is_empty());
    }

    #[test]
    fn a_verdict_that_is_already_settled_reads_no_files() {
        // On the volumes that give a fallback there is no id to read, so a
        // probe describes the file instead — which means reading all of it.
        // Probing a folder for a verdict that cannot change is one deletion
        // costing a read of every note beside it, over a network share.
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        for name in ["one.md", "two.md", "three.md"] {
            fs::write(dir.path().join(name), b"body").unwrap();
        }

        let asked: Arc<Mutex<Vec<PathBuf>>> = Arc::new(Mutex::new(Vec::new()));
        let (tracking, _files) = tracking_with(RecordingFiles {
            identity: Some(FileIdentity::Fallback {
                path: gone.to_string_lossy().into_owned(),
                size: 4,
                mtime_ms: None,
                hash: writ_core::hash::sha256_bytes(b"body"),
            }),
            ..RecordingFiles::default()
        });
        let tracking = FileTracking {
            probe: Arc::new(CountingProbe {
                asked: asked.clone(),
            }),
            files: tracking.files.clone(),
        };
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &holding(),
                batch: &[dir.path().join("one.md")],
                tracking: &tracking,
            },
        );

        assert!(
            asked.lock().unwrap().is_empty(),
            "a settled verdict asked the filesystem anyway: {:?}",
            asked.lock().unwrap()
        );
        assert!(
            matches!(
                event,
                Some(WritEvent::BufferExternal {
                    change: ExternalChange::Modified,
                    ..
                })
            ),
            "got {event:?}"
        );
    }

    #[test]
    fn a_note_with_nothing_recorded_about_its_file_is_told_it_is_gone() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");

        let (tracking, files) = tracking_with(RecordingFiles::default());
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &holding(),
                batch: &[],
                tracking: &tracking,
            },
        );

        assert!(matches!(
            event,
            Some(WritEvent::BufferExternal {
                change: ExternalChange::Removed,
                ..
            })
        ));
        assert_eq!(files.removed.lock().unwrap().len(), 1);
    }

    #[test]
    fn news_a_tab_already_has_is_not_sent_twice() {
        // One file leaving one folder can be seen by both watchers. The record
        // is what makes the second one silent.
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");

        let (tracking, files) = tracking_with(RecordingFiles {
            already_told: Arc::new(Mutex::new(true)),
            ..RecordingFiles::default()
        });
        assert!(open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &holding(),
                batch: &[],
                tracking: &tracking,
            },
        )
        .is_none());
        assert_eq!(files.removed.lock().unwrap().len(), 1);
    }

    #[test]
    fn a_file_that_is_there_again_tells_the_record_before_the_tab() {
        let dir = tempdir().unwrap();
        let back = dir.path().join("back.md");
        fs::write(&back, b"body").unwrap();

        let (tracking, files) = tracking_with(RecordingFiles::default());
        let batch = vec![back.clone()];
        let event = classify_open_file_event(
            &back,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(matches!(
            event,
            Some(WritEvent::BufferExternal {
                change: ExternalChange::Modified,
                ..
            })
        ));
        assert_eq!(
            files.returned.lock().unwrap().as_slice(),
            &[back],
            "a file that came back has to clear the mark, or the tab keeps refusing to write"
        );
    }

    /// The event `path`'s file becomes for a tab that last read `loaded` from
    /// it, with the removal mark the record would answer.
    fn modification_for(
        path: &Path,
        loaded: &[u8],
        was_removed: bool,
    ) -> (Option<WritEvent>, Arc<RecordingFiles>) {
        let (tracking, files) = tracking_with(RecordingFiles {
            last: Some(last_read(loaded)),
            was_removed,
            ..RecordingFiles::default()
        });
        let batch = vec![path.to_path_buf()];
        let event = classify_open_file_event(
            path,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );
        (event, files)
    }

    #[test]
    fn a_report_carrying_the_bytes_the_tab_loaded_is_not_news() {
        // The write that seeded a file can be delivered after Writ opened and
        // read it: FSEvents coalesces and delivers on its own schedule, and on
        // a loaded machine the seed lands behind the open. Handing it to the
        // tab shows the user an external-change notice for the bytes they are
        // looking at.
        let dir = tempdir().unwrap();
        let path = dir.path().join("seeded.md");
        fs::write(&path, b"as another program left it").unwrap();

        let (event, files) = modification_for(&path, b"as another program left it", false);

        assert!(event.is_none(), "saw {event:?}");
        assert_eq!(
            files.returned.lock().unwrap().as_slice(),
            &[path],
            "the id is re-read whether or not the tab is told, or the next rename reads as a deletion"
        );
    }

    #[test]
    fn a_report_carrying_bytes_the_tab_has_not_read_is_news() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("edited.md");
        fs::write(&path, b"somebody else wrote this").unwrap();

        let (event, _files) = modification_for(&path, b"what the tab loaded", false);

        assert!(matches!(
            event,
            Some(WritEvent::BufferExternal {
                change: ExternalChange::Modified,
                ..
            })
        ));
    }

    #[test]
    fn a_file_back_from_the_trash_is_news_holding_the_same_bytes() {
        // The tab is refusing to save while the file is marked gone, so being
        // at its path again is the news, not the bytes.
        let dir = tempdir().unwrap();
        let path = dir.path().join("restored.md");
        fs::write(&path, b"body").unwrap();

        let (event, _files) = modification_for(&path, b"body", true);

        assert!(matches!(
            event,
            Some(WritEvent::BufferExternal {
                change: ExternalChange::Modified,
                ..
            })
        ));
    }

    #[test]
    fn the_notes_folder_route_swallows_the_same_report() {
        // A file inside the notes folder reaches its tab through the notes
        // watcher instead, and one route staying quiet is no use if the other
        // one talks.
        let dir = tempdir().unwrap();
        let path = dir.path().join("seeded.md");
        fs::write(&path, b"as another program left it").unwrap();

        let (tracking, _files) = tracking_with(RecordingFiles {
            last: Some(last_read(b"as another program left it")),
            ..RecordingFiles::default()
        });
        let batch = vec![path.clone()];
        let event = open_note_change(
            "note-1",
            &path,
            false,
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "saw {event:?}");
    }

    #[test]
    fn a_files_own_path_is_never_a_candidate_for_where_it_went() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        let other = dir.path().join("other.md");
        fs::write(&other, b"x").unwrap();

        let candidates = candidates_for(&gone, &[gone.clone(), other.clone()], None);
        assert!(!candidates.all().any(|c| c == &gone));
        assert!(candidates.all().any(|c| c == &other));
        assert_eq!(
            candidates.all().filter(|c| *c == &other).count(),
            1,
            "the batch and the folder listing both name it; it is one candidate"
        );
    }

    /// What `open_note_vanished` decided, as the change and the path it named.
    fn verdict_of(event: &Option<WritEvent>) -> Option<(&ExternalChange, Option<&str>)> {
        match event {
            Some(WritEvent::BufferExternal {
                change, new_path, ..
            }) => Some((change, new_path.as_deref())),
            _ => None,
        }
    }

    #[test]
    fn a_rewrite_nobody_reported_does_not_turn_a_rename_into_a_deletion() {
        // Two writes in one window are reported as one. A program rewrote the
        // file — a sibling temp renamed over it, which is how every editor and
        // every sync client writes — and then renamed it, so the only event is
        // the path going empty and the id on record is the one the rewrite
        // retired. Nothing carries it, and the tab would mark itself removed
        // over a file sitting at its new path with the user's text in it.
        let dir = tempdir().unwrap();
        let from = dir.path().join("before.md");
        let to = dir.path().join("after.md");
        fs::write(&from, b"text worth keeping").unwrap();
        let retired = crate::watcher::identity::read_identity(&from);
        let temp = dir.path().join("before.md.other-program-tmp");
        fs::write(&temp, b"text worth keeping").unwrap();
        fs::rename(&temp, &from).unwrap();
        fs::rename(&from, &to).unwrap();

        let (tracking, files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"text worth keeping")),
            ..RecordingFiles::default()
        });
        let batch = vec![temp, from.clone(), to.clone()];
        let event = open_note_vanished(
            "note-1",
            &from,
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert_eq!(
            verdict_of(&event),
            Some((&ExternalChange::Moved, to.to_str())),
            "the bytes the tab last read are at the new path; that is the file"
        );
        assert!(files.removed.lock().unwrap().is_empty());
    }

    #[test]
    fn a_deletion_beside_an_unrelated_creation_is_still_a_deletion() {
        // The shape a batch takes when someone deletes one note and creates
        // another, which is also what a branch checkout does. Pairing the two
        // on nothing but their being in one window would put the tab on a file
        // it has never read and let the next save write over it.
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        let unrelated = dir.path().join("unrelated.md");
        fs::write(&gone, b"text worth keeping").unwrap();
        // Written before the deletion, so the unrelated note cannot inherit
        // the deleted file's inode number on a filesystem that reuses them.
        fs::write(&unrelated, b"somebody else's note").unwrap();
        let retired = crate::watcher::identity::read_identity(&gone);
        fs::remove_file(&gone).unwrap();

        let (tracking, _files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"text worth keeping")),
            ..RecordingFiles::default()
        });
        let batch = vec![gone.clone(), unrelated];
        let pending = holding();
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &pending,
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "a removal waits before it is announced");
        assert_eq!(
            verdict_of(&announced_after_the_wait(&pending, &tracking).pop()),
            Some((&ExternalChange::Removed, None))
        );
    }

    #[test]
    fn only_the_batch_is_read_for_a_match_on_bytes() {
        // Ids are read from the folder listing too; bytes are not. Hashing the
        // folder a note left reads every note in it, and on a share that is
        // one deletion pulling the whole folder over the network.
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        let twin = dir.path().join("twin.md");
        fs::write(&gone, b"text worth keeping").unwrap();
        // Same reason as above: the twin exists before the deletion, so it
        // carries an inode number of its own on every filesystem.
        fs::write(&twin, b"text worth keeping").unwrap();
        let retired = crate::watcher::identity::read_identity(&gone);
        fs::remove_file(&gone).unwrap();

        let (tracking, _files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"text worth keeping")),
            ..RecordingFiles::default()
        });
        let batch = vec![gone.clone()];
        let pending = holding();
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                hold: &pending,
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "a removal waits before it is announced");
        assert_eq!(
            verdict_of(&announced_after_the_wait(&pending, &tracking).pop()),
            Some((&ExternalChange::Removed, None)),
            "and the folder listing is still no place to match bytes"
        );
    }

    #[test]
    fn a_note_replaced_by_a_folder_of_the_same_name_reads_as_a_file_that_went() {
        // A path holding a directory holds no note. Dropping the event left
        // the tab carrying a dead id and saving into a raw `Is a directory`
        // rather than saying the file is gone.
        let dir = tempdir().unwrap();
        let path = dir.path().join("note.md");
        fs::write(&path, b"body").unwrap();
        let retired = crate::watcher::identity::read_identity(&path);
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();

        let (tracking, _files) = tracking_with(RecordingFiles {
            identity: retired,
            last: Some(last_read(b"body")),
            ..RecordingFiles::default()
        });
        let batch = vec![path.clone()];
        let pending = holding();
        let event = classify_open_file_event(
            &path,
            "note-1",
            &crate::watcher::handler::create_ignore_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                hold: &pending,
                batch: &batch,
                tracking: &tracking,
            },
        );

        assert!(event.is_none(), "a removal waits before it is announced");
        assert_eq!(
            verdict_of(&announced_after_the_wait(&pending, &tracking).pop()),
            Some((&ExternalChange::Removed, None)),
            "a folder standing where the file was is not the file coming back"
        );
    }

    #[test]
    fn candidates_come_in_a_fixed_order_rather_than_the_filesystems() {
        // A hard link is one file with two names, so the order decides which
        // name a tab lands on. Leaving it to `read_dir` leaves it to the
        // filesystem, and two machines holding the same folder could answer
        // differently.
        let notes = tempdir().unwrap();
        let outside = tempdir().unwrap();
        let gone = notes.path().join("gone.md");
        let batch = vec![
            notes.path().join("second.md"),
            outside.path().join("elsewhere.md"),
            notes.path().join("first.md"),
        ];

        let candidates = candidates_for(&gone, &batch, Some(notes.path()));

        assert_eq!(
            candidates.batch,
            vec![
                notes.path().join("first.md"),
                notes.path().join("second.md"),
                outside.path().join("elsewhere.md"),
            ]
        );
    }

    #[test]
    fn the_events_one_write_raises_on_linux_tell_the_tab_once() {
        // Classifying an event opens the file, and on Linux opening a file
        // inside a watched folder raises another event for it. Delivered one
        // per batch, that told the tab the same thing eleven times over.
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        fs::write(&file, b"rewritten by another program\n").unwrap();

        let ignore = make_set();
        let now = Instant::now();
        let batch = vec![file.clone()];
        let tracking = FileTracking::untracked();
        let vanished = VanishedContext {
            hold: &holding(),
            batch: &batch,
            tracking: &tracking,
        };
        let ungated = (0..11)
            .filter(|_| {
                classify_open_file_event(
                    &file,
                    "note-1",
                    &make_set(),
                    DEFAULT_IGNORE_TTL,
                    now,
                    &vanished,
                )
                .is_some()
            })
            .count();
        assert_eq!(
            ungated, 11,
            "the burst is what classification alone reports"
        );

        let mut seen = LastSeen::new();
        let told: Vec<WritEvent> = (0..11)
            .filter_map(|_| {
                report_open_file_event(
                    &file,
                    "note-1",
                    &ignore,
                    &mut seen,
                    DEFAULT_IGNORE_TTL,
                    now,
                    &vanished,
                )
            })
            .collect();

        assert_eq!(told.len(), 1, "the tab must be told once, saw {told:?}");
    }

    #[test]
    fn a_second_write_reaches_the_tab_however_recently_the_first_did() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        fs::write(&file, b"first\n").unwrap();

        let ignore = make_set();
        let mut seen = LastSeen::new();
        let now = Instant::now();
        let batch = vec![file.clone()];
        let tracking = FileTracking::untracked();
        let vanished = VanishedContext {
            hold: &holding(),
            batch: &batch,
            tracking: &tracking,
        };
        assert!(report_open_file_event(
            &file,
            "note-1",
            &ignore,
            &mut seen,
            DEFAULT_IGNORE_TTL,
            now,
            &vanished
        )
        .is_some());
        assert!(report_open_file_event(
            &file,
            "note-1",
            &ignore,
            &mut seen,
            DEFAULT_IGNORE_TTL,
            now,
            &vanished
        )
        .is_none());

        fs::write(&file, b"second, and longer\n").unwrap();
        assert!(
            report_open_file_event(
                &file,
                "note-1",
                &ignore,
                &mut seen,
                DEFAULT_IGNORE_TTL,
                now,
                &vanished
            )
            .is_some(),
            "a file written again must reach its tab"
        );
    }

    #[test]
    fn another_program_rewriting_an_open_file_surfaces_with_what_it_now_holds() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("shared.md");
        fs::write(&file, b"written by somebody else\n").unwrap();

        let (batch, tracking) = nothing_tracked(&file);
        match classify_open_file_event(
            &file,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
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
            classify_open_file_event(
                &file,
                "note-1",
                &set,
                DEFAULT_IGNORE_TTL,
                now,
                &VanishedContext {
                    hold: &holding(),
                    batch: &nothing_tracked(&file).0,
                    tracking: &FileTracking::untracked(),
                },
            )
            .is_none(),
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
            classify_open_file_event(
                &file,
                "note-1",
                &set,
                DEFAULT_IGNORE_TTL,
                now,
                &VanishedContext {
                    hold: &holding(),
                    batch: &nothing_tracked(&file).0,
                    tracking: &FileTracking::untracked(),
                },
            )
            .is_some(),
            "an edit landing right after writ's own save must still surface"
        );
    }

    #[test]
    fn a_file_that_has_gone_surfaces_as_removed_with_no_digest() {
        let dir = tempdir().unwrap();
        let file = dir.path().join("gone.md");

        let (batch, tracking) = nothing_tracked(&file);
        match classify_open_file_event(
            &file,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                hold: &holding(),
                batch: &batch,
                tracking: &tracking,
            },
        ) {
            Some(WritEvent::BufferExternal {
                change, disk_hash, ..
            }) => {
                assert_eq!(change, ExternalChange::Removed);
                assert_eq!(disk_hash, None);
            }
            other => panic!("expected a removal, got {other:?}"),
        }
    }

    #[test]
    fn moving_the_notes_folder_leaves_the_folders_already_watched_alone() {
        let notes = tempdir().unwrap();
        let moved_to = tempdir().unwrap();
        let repo = tempdir().unwrap();
        let file = repo.path().join("one.md");
        fs::write(&file, b"x").unwrap();

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
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

    #[test]
    fn a_folder_the_notes_root_moved_onto_gives_up_its_own_watch() {
        // Two watchers over one folder report every change in it twice. The
        // notes watcher is recursive and arrives with the new root, so this
        // one steps back.
        let notes = tempdir().unwrap();
        let elsewhere = tempdir().unwrap();
        let file = elsewhere.path().join("one.md");
        fs::write(&file, b"x").unwrap();

        let Harness {
            mut registry,
            native_released: released,
            ..
        } = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &file);
        assert_eq!(registry.watched_dirs().len(), 1);

        registry.set_notes_root(elsewhere.path());

        assert!(
            registry.watched_dirs().is_empty(),
            "the notes watcher covers it now"
        );
        assert_eq!(released.lock().unwrap().len(), 1);
        assert_eq!(
            registry.note_at(&file).as_deref(),
            Some("note-1"),
            "the tab is still open and the notes watcher has to find it"
        );
    }

    #[test]
    fn a_folder_the_notes_root_moved_away_from_takes_a_watch_of_its_own() {
        // The regression this closes: a tab open on a file in the old notes
        // folder was covered by the notes watcher and never registered here.
        // Moving the folder left it watched by nothing, and the tab heard
        // nothing about its file until it was closed and reopened.
        let notes = tempdir().unwrap();
        let moved_to = tempdir().unwrap();
        let note = notes.path().join("today.md");
        fs::write(&note, b"x").unwrap();

        let Harness {
            mut registry,
            native_watched: native_log,
            ..
        } = registry_with(false, false, notes.path());

        assert_eq!(
            registry.watch_parent_of("note-1", &note),
            WatchOutcome::AlreadyCovered
        );
        assert!(native_log.lock().unwrap().is_empty());

        registry.set_notes_root(moved_to.path());

        assert_eq!(
            registry.kind(notes.path()),
            Some(WatcherKind::Native),
            "the folder the notes left needs a watcher of its own"
        );
        assert_eq!(native_log.lock().unwrap().len(), 1);
        assert_eq!(registry.note_at(&note).as_deref(), Some("note-1"));
    }

    #[test]
    fn a_folder_the_notes_root_moved_away_from_that_cannot_be_watched_is_dropped() {
        let notes = tempdir().unwrap();
        let moved_to = tempdir().unwrap();
        let note = notes.path().join("today.md");
        fs::write(&note, b"x").unwrap();

        let Harness { mut registry, .. } = registry_with(true, true, notes.path());
        registry.watch_parent_of("note-1", &note);

        registry.set_notes_root(moved_to.path());

        assert!(registry.watched_dirs().is_empty());
        assert_eq!(
            registry.note_at(&note),
            None,
            "a folder nothing will watch is not recorded as one that is"
        );

        // The drop leaves nothing that claims coverage. Asking again goes back
        // to the backends rather than trusting a record of a folder no longer
        // held, and closing the tab afterwards finds nothing to release.
        assert_eq!(
            registry.watch_parent_of("note-1", &note),
            WatchOutcome::Unwatchable
        );
        registry.unwatch_parent_of("note-1");
        assert!(registry.watched_dirs().is_empty());
    }

    #[test]
    fn a_note_inside_the_notes_folder_is_still_findable_by_its_path() {
        // The notes watcher owns the watch on this folder and asks the
        // registry which tab a changed file belongs to. Recording only the
        // folders this registry armed is what left a change inside the notes
        // folder with no tab to deliver it to.
        let notes = tempdir().unwrap();
        let note = notes.path().join("today.md");
        let other = notes.path().join("untitled.md");
        fs::write(&note, b"x").unwrap();
        fs::write(&other, b"x").unwrap();

        let Harness { mut registry, .. } = registry_with(false, false, notes.path());
        registry.watch_parent_of("note-1", &note);

        assert_eq!(registry.note_at(&note).as_deref(), Some("note-1"));
        assert_eq!(
            registry.note_at(&other),
            None,
            "a file in the notes folder that no tab holds is still not an open note"
        );
        assert!(
            registry.watched_dirs().is_empty(),
            "findable is not the same as watched here"
        );

        registry.unwatch_parent_of("note-1");
        assert_eq!(registry.note_at(&note), None);
    }
}
