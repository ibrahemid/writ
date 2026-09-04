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
use writ_core::notes::identity::{classify_delete, DeleteVerdict, FileIdentity};
use writ_core::watcher::change_event::ExternalChange;
use writ_core::watcher::ignore::{SuppressDecision, DEFAULT_IGNORE_TTL};

use super::handler::{ignore_key_path, IgnoreSet};
use super::moves::FileTracking;

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
            // A rename arrives as the old path leaving and the new one
            // appearing in the same window, so the batch is where a file that
            // moved is found again.
            let batch: Vec<PathBuf> = events.iter().map(|event| event.path.clone()).collect();
            for event in &events {
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
                    &VanishedContext {
                        batch: &batch,
                        tracking: &tracking,
                    },
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
    vanished: &VanishedContext<'_>,
) -> Option<WritEvent> {
    if !path.exists() {
        return open_note_vanished(note_id, path, vanished);
    }
    if !std::fs::metadata(path).is_ok_and(|m| m.is_file()) {
        return None;
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

    vanished.tracking.files.note_file_returned(note_id, path);
    Some(open_note_modified(note_id, path, current_bytes.as_deref()))
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

/// The paths that could be holding the file that left `path`.
///
/// The batch first, then the folder the file left, and the file's own path is
/// never a candidate for itself.
///
/// Every path this produces is one a watcher covers: the batch is one
/// watcher's own window, and the folder is the one the file left, which is
/// watched because a tab's file was in it. That invariant is what makes a
/// match here safe to follow — the tab lands somewhere its changes still reach
/// it. A candidate source that broke it would have to be checked against the
/// watched folders before it could be believed (ADR-033 §11).
pub fn candidates_for(path: &Path, batch: &[PathBuf]) -> Vec<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut consider = |candidate: PathBuf, out: &mut Vec<PathBuf>| {
        if candidate == path || !seen.insert(candidate.clone()) {
            return;
        }
        out.push(candidate);
    };
    for candidate in batch {
        consider(candidate.clone(), &mut candidates);
    }
    if let Some(dir) = path.parent() {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten().take(MAX_FOLDER_CANDIDATES) {
                consider(entry.path(), &mut candidates);
            }
        }
    }
    candidates
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
    let candidates: Vec<(PathBuf, FileIdentity)> = if before.is_durable() {
        candidates_for(path, vanished.batch)
            .into_iter()
            .filter_map(|candidate| {
                let identity = vanished.tracking.probe.identity_of(&candidate)?;
                Some((candidate, identity))
            })
            .collect()
    } else {
        Vec::new()
    };

    match classify_delete(&before, &candidates) {
        DeleteVerdict::Moved(to) => files
            .note_file_moved(note_id, path, &to)
            .then(|| open_note_moved(note_id, path, &to)),
        DeleteVerdict::Removed => files
            .note_file_removed(note_id, path)
            .then(|| open_note_removed(note_id, path)),
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
    vanished.tracking.files.note_file_returned(note_id, path);
    Some(open_note_modified(
        note_id,
        path,
        readable_bytes(path).as_deref(),
    ))
}

#[cfg(test)]
mod tests {
    use super::super::moves::NoteFiles;
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
        moved: Arc<Mutex<Vec<(PathBuf, PathBuf)>>>,
        removed: Arc<Mutex<Vec<PathBuf>>>,
        returned: Arc<Mutex<Vec<PathBuf>>>,
        already_told: Arc<Mutex<bool>>,
    }

    impl NoteFiles for RecordingFiles {
        fn identity_of(&self, _note_id: &str) -> Option<FileIdentity> {
            self.identity.clone()
        }

        fn note_file_moved(&self, _note_id: &str, from: &Path, to: &Path) -> bool {
            self.moved
                .lock()
                .unwrap()
                .push((from.to_path_buf(), to.to_path_buf()));
            !*self.already_told.lock().unwrap()
        }

        fn note_file_removed(&self, _note_id: &str, path: &Path) -> bool {
            self.removed.lock().unwrap().push(path.to_path_buf());
            !*self.already_told.lock().unwrap()
        }

        fn note_file_returned(&self, _note_id: &str, path: &Path) -> bool {
            self.returned.lock().unwrap().push(path.to_path_buf());
            true
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
        let identity = crate::watcher::identity::read_identity(&gone);
        fs::remove_file(&gone).unwrap();
        for name in ["other.md", "third.md"] {
            fs::write(dir.path().join(name), b"still here").unwrap();
        }

        let (tracking, files) = tracking_with(RecordingFiles {
            identity,
            ..RecordingFiles::default()
        });
        let batch = vec![gone.clone()];
        let event = open_note_vanished(
            "note-1",
            &gone,
            &VanishedContext {
                batch: &batch,
                tracking: &tracking,
            },
        );

        match event {
            Some(WritEvent::BufferExternal { change, .. }) => {
                assert_eq!(change, ExternalChange::Removed);
            }
            other => panic!("expected a removal, got {other:?}"),
        }
        assert_eq!(files.removed.lock().unwrap().as_slice(), &[gone]);
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

    #[test]
    fn a_files_own_path_is_never_a_candidate_for_where_it_went() {
        let dir = tempdir().unwrap();
        let gone = dir.path().join("gone.md");
        let other = dir.path().join("other.md");
        fs::write(&other, b"x").unwrap();

        let candidates = candidates_for(&gone, &[gone.clone(), other.clone()]);
        assert!(!candidates.contains(&gone));
        assert!(candidates.contains(&other));
        assert_eq!(
            candidates.iter().filter(|c| *c == &other).count(),
            1,
            "the batch and the folder listing both name it; it is one candidate"
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
    fn a_folder_appearing_where_a_note_was_is_not_a_change_to_the_note() {
        let dir = tempdir().unwrap();
        let sub = dir.path().join("subfolder");
        fs::create_dir(&sub).unwrap();

        assert!(classify_open_file_event(
            &sub,
            "note-1",
            &make_set(),
            DEFAULT_IGNORE_TTL,
            Instant::now(),
            &VanishedContext {
                batch: &nothing_tracked(&sub).0,
                tracking: &FileTracking::untracked(),
            },
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
