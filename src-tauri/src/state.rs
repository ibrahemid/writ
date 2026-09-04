use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex, RwLock};
use tracing::{info, warn};
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::recovery::RecoveredBuffer;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::consistency::ConsistencyChecker;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

use writ_storage::layout_state::LayoutStateStore;
use writ_storage::notes_index::NotesIndexStore;

use crate::fts_scheduler::FtsScheduler;
use crate::poison::recover_poison;
use crate::preview::handler::RenderCache;
use crate::quit::QuitState;
use crate::security::{canonicalize_for_authorization, canonicalize_root, AuthorizedPaths};
use crate::watcher::handler::{IgnoreSet, WatcherHandle};

/// What Writ last saw on disk for one note.
///
/// The record is the policy's ([`writ_core::notes::guard`]); the map below is
/// where this process keeps one per open tab.
pub use writ_core::notes::guard::DiskState;

/// Reads `path`'s metadata to complete a record around an already computed
/// digest, falling back to `size` when the metadata is gone.
fn disk_state_of(path: &Path, hash: writ_core::hash::Sha256Digest, size: u64) -> DiskState {
    let metadata = std::fs::metadata(path).ok();
    DiskState {
        hash,
        size: metadata.as_ref().map(|m| m.len()).unwrap_or(size),
        mtime: metadata.as_ref().and_then(|m| m.modified().ok()),
    }
}

/// Why startup could not keep the notes folder it was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NotesRootFallbackReason {
    /// The folder could not be created: it names a file, a volume that is not
    /// mounted, a path the OS denies, or one written relative to a working
    /// directory a launched app does not have.
    Unusable,
    /// The folder is Writ's own data folder, holds it, or sits inside it, so
    /// the notes and the database would share a folder
    /// ([`writ_core::notes::refuse_notes_root`]).
    HoldsWritData,
}

/// The notes folder startup was asked for and did not keep.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct NotesRootFallback {
    /// The folder as it was configured, in its own spelling.
    pub from: String,
    /// Why the notes are somewhere else.
    pub reason: NotesRootFallbackReason,
}

pub struct AppState {
    pub store: Mutex<BufferStore>,
    pub config_store: ConfigStore,
    pub config: Mutex<WritConfig>,
    pub writ_dir: PathBuf,
    pub buffers_dir: PathBuf,
    /// Canonical root of the notes folder. Always exists; created at startup.
    ///
    /// Replaced in place when the user moves the folder from Settings, which
    /// is the one thing that changes it while Writ runs. Read it through
    /// [`AppState::notes_root`] rather than holding the guard: every write
    /// gate and every note command asks for it, and a held read guard would
    /// block the move for as long as the caller lives.
    pub notes_root: RwLock<PathBuf>,
    /// The configured notes folder startup could not use, when it fell back to
    /// the default one. `None` on every ordinary launch. The Settings surface
    /// reads this to tell the user which folder was turned down, why, and
    /// where the notes went instead.
    ///
    /// Behind a lock beside [`AppState::notes_root`] and cleared by
    /// [`AppState::set_notes_root`], because it describes the folder the
    /// settings name: once the user has moved the notes somewhere Writ kept,
    /// the settings name that folder and there is nothing left to say. Read it
    /// through [`AppState::notes_root_fallback`].
    pub notes_root_fallback: RwLock<Option<NotesRootFallback>>,
    pub watcher_ignore: IgnoreSet,
    pub watcher: Mutex<Option<WatcherHandle>>,
    /// The recursive watcher over the notes folder. Held here so it lives as
    /// long as the application; dropping it stops the watch.
    pub notes_watcher: Mutex<Option<WatcherHandle>>,
    pub pending_opens: Mutex<Vec<String>>,
    pub frontend_ready: AtomicBool,
    pub transforms: RwLock<TransformRegistry>,
    pub event_bus: Arc<EventBus>,
    pub update_phase: Mutex<UpdatePhase>,
    pub authorized_paths: AuthorizedPaths,
    pub preview_registry: Arc<RwLock<ContentRendererRegistry>>,
    pub preview_render_cache: Arc<RenderCache>,
    /// Per-buffer preview layout persistence. Holds its own SQLite
    /// connection (WAL permits concurrent connections to the same file).
    pub layout_state: LayoutStateStore,
    /// The path-keyed index over the notes folder (ADR-028 section 7). Holds
    /// its own connection so the startup reconcile never queues behind a save
    /// and a keystroke never queues behind the reconcile.
    pub notes_index: Arc<NotesIndexStore>,
    /// Set when the reconcile thread should stop, which is shutdown. The
    /// thread polls it per entry, so a quit during a large walk does not wait
    /// for the walk.
    pub notes_index_cancel: Arc<AtomicBool>,
    /// How far the shutdown path has got, and whether the frontend has
    /// answered [`writ_core::events::bus::WritEvent::FlushBeforeQuit`] by
    /// writing everything it was holding inside the autosave debounce window.
    /// The shutdown path waits on that answer, but only as far as
    /// [`writ_core::recovery::QUIT_FLUSH_TIMEOUT`].
    pub quit: Arc<QuitState>,
    /// Buffers restored from the crash snapshot on this launch.
    /// Consumed by the `get_recovered_buffers` command and cleared.
    pub recovered_buffers: Mutex<Vec<RecoveredBuffer>>,
    /// `true` when the previous session ended without a clean snapshot.
    /// Recovery itself is pull-based via `recovered_buffers`; this flag
    /// records the detection for diagnostics and test assertions.
    pub was_dirty_shutdown: bool,
    /// Canonical root of the open workspace folder, if any.
    pub workspace_root: Mutex<Option<PathBuf>>,
    /// Live workspace directory watcher; replaced when the root changes.
    pub workspace_watcher: Mutex<Option<WatcherHandle>>,
    /// Canonical root of the watched inbox folder, if any (ADR-018).
    pub inbox_root: Mutex<Option<PathBuf>>,
    /// Live inbox watcher; replaced when the inbox path changes.
    pub inbox_watcher: Mutex<Option<WatcherHandle>>,
    /// Coalesces deferred FTS reindexes off the autosave path (ADR-020).
    pub fts_scheduler: FtsScheduler,
    /// In-memory workspace file-name index (ADR-026). Shared with the watcher
    /// subscriber and the background build thread.
    pub workspace_index: crate::workspace_index::SharedIndex,
    /// Monotonic content-search generation. Each `search_workspace_content`
    /// call bumps it and captures its value; the walker's cancel closure
    /// compares against this so a newer query stops the older one (ADR-026).
    pub search_generation: Arc<AtomicU64>,
    /// What a buffer's file held at the moment Writ last read or wrote it,
    /// keyed by buffer id.
    ///
    /// Kept here for now, in memory only: a relaunch starts blank, which is
    /// no worse than a relaunch already was. A later change moves this
    /// record into the store as `last_known_disk_hash` so it survives one,
    /// which is the name to reach for if this field is being generalised
    /// rather than just relocated. An entry lives as long as the tab does:
    /// closing or deleting a note drops it, so a note reopened weeks later
    /// is compared against the file, not against a stale record of it.
    pub last_disk_hash: Mutex<HashMap<String, DiskState>>,
}

impl AppState {
    pub fn initialize() -> Result<Self, Box<dyn std::error::Error>> {
        let data_dir_override = std::env::var("WRIT_DATA_DIR")
            .ok()
            .filter(|value| !value.trim().is_empty());
        let writ_dir = resolve_writ_dir(data_dir_override.clone(), dirs::home_dir())?;

        std::fs::create_dir_all(&writ_dir)?;

        let buffers_dir = writ_dir.join("buffers");
        std::fs::create_dir_all(&buffers_dir)?;

        let db_path = writ_dir.join("writ.db");
        let conn = open_database(&db_path)?;
        run_migrations(&conn)?;
        info!(path = %db_path.display(), "database initialized");

        // Second connection for layout-state persistence; migrations have
        // already created the table on the primary connection above.
        let layout_state = LayoutStateStore::new(open_database(&db_path)?);
        // Third connection, for the notes index, on the same grounds.
        let notes_index = Arc::new(NotesIndexStore::open(&db_path)?);

        let config_path = writ_dir.join("config.toml");
        let config_store = ConfigStore::new(config_path);
        let config = config_store.read()?;
        info!("config loaded");

        // The notes folder is where every note Writ mints lands, and the file
        // it lands in is the only copy of the text (ADR-028). It is resolved,
        // created and canonicalised before anything can write into it, so the
        // containment check the write gate runs compares two canonical paths.
        // An instance running against its own data folder keeps its notes
        // there too, so a dev or recording launch never creates the folder the
        // user reads.
        let notes_env_override = std::env::var("WRIT_NOTES_DIR").ok();
        let home = dirs::home_dir();
        let (notes_root, notes_root_fallback) = resolve_and_create_notes_root(
            writ_core::notes::NotesRootSources {
                env_override: notes_env_override.as_deref(),
                configured: config.notes.root.as_deref(),
                data_dir: data_dir_override.as_ref().map(|_| writ_dir.as_path()),
                home: home.as_deref(),
            },
            &writ_dir,
        )?;
        info!(path = %notes_root.display(), "notes folder ready");

        // The half of the data-folder guard that needs both paths. The sync
        // provider half already ran in `run()`, before the database was
        // opened; this one can only run once the notes folder is resolved.
        // `usable_notes_root` carries the same overlap rule and diverts first,
        // so this is the invariant behind that rule; `run()` reports the
        // refusal under `StartupStage::DataDirectoryLocation`.
        let verdict =
            crate::startup::data_dir_verdict(&writ_dir, home.as_deref(), Some(&notes_root));
        if verdict != writ_core::startup::DataDirVerdict::Ok {
            return Err(Box::new(writ_core::startup::DataDirRefused(verdict)));
        }

        let mut store = BufferStore::new(conn, buffers_dir.clone());
        // A save is stamped into the watcher's ignore set before it lands, so
        // the notes watcher never sees Writ's own writes: the store indexes
        // them itself, and needs to know which folder is the notes folder to
        // tell a note from an external file someone opened.
        store.set_notes_root(notes_root.clone());

        // Normalize legacy mirror filenames to `{id}.txt` and install the
        // UNIQUE(filename) index before any other store operation reads or
        // writes a backing file (audit blocker #53.7).
        match store.reconcile_buffer_filenames() {
            Ok(0) => {}
            Ok(count) => info!(count, "reconciled legacy buffer filenames at startup"),
            Err(e) => warn!(error = %e, "failed to reconcile buffer filenames"),
        }

        // Recovery must run before reclaim_empty_scratch: a note that crashed
        // before its autosave flushed holds nothing on disk but has text in
        // the snapshot, and reclaiming first would delete it. A note that
        // never reached a file gets one here, minted by the same policy the
        // first keystroke would have used, because there is nowhere else the
        // text can go now (ADR-028 §1).
        let was_dirty_shutdown = store.is_dirty_shutdown().unwrap_or(false);
        // What each recovered note's file holds once recovery is done, which
        // seeds the write guard. Without it the first save after an unclean
        // relaunch would have nothing to compare against and would write over
        // whatever arrived while Writ was down.
        let mut recovered_disk_states: HashMap<String, DiskState> = HashMap::new();
        let recovered_buffers = if was_dirty_shutdown {
            info!("dirty shutdown detected; resolving recovery");
            let recovered = store.resolve_recovery().unwrap_or_default();
            info!(count = recovered.len(), "buffers eligible for recovery");
            for buf in &recovered {
                match restore_recovered_buffer(&store, &notes_root, buf) {
                    // Nothing is recorded for a note whose file was never
                    // read, so its first save reads it rather than trusting a
                    // record nobody took.
                    Ok(Some(state)) => {
                        recovered_disk_states.insert(buf.id.clone(), state);
                    }
                    Ok(None) => {}
                    Err(e) => warn!(buffer_id = %buf.id, error = %e, "recovery write failed"),
                }
            }
            recovered
        } else {
            Vec::new()
        };

        // Every note becomes a file (ADR-028 §4). After recovery, so the text
        // the last session never flushed is in place before the pass reads
        // it; before reclaim, so no row it is about to write gets deleted
        // underneath it.
        let archive_root = writ_dir.join("archive");
        let piped_root = writ_dir.join("piped");
        match writ_storage::notes_migration::run_notes_migration(
            &store,
            writ_storage::notes_migration::MigrationRoots {
                db_path: &db_path,
                notes: &notes_root,
                archive: &archive_root,
                piped: &piped_root,
            },
            chrono::Utc::now(),
        ) {
            Ok(report) => info!(
                migrated = report.migrated,
                archived = report.archived,
                recovered = report.recovered,
                piped = report.piped,
                failed = report.failed,
                deleted_empty = report.deleted_empty,
                "notes migration finished"
            ),
            Err(e) => warn!(error = %e, "notes migration failed"),
        }

        // Counted on every launch, including the one that took the copy: the
        // counter starts at 0. A failure here is a line in the log, never a
        // reason a launch cannot open.
        match store.age_out_rollback_copy(writ_storage::rollback::ROLLBACK_KEEP_LAUNCHES) {
            Ok(true) => info!("deleted the database copy taken before the notes migration"),
            Ok(false) => {}
            Err(e) => warn!(error = %e, "could not age out the pre-migration database copy"),
        }

        match store.reclaim_empty_scratch() {
            Ok(0) => {}
            Ok(count) => info!(count, "reclaimed empty scratch buffers at startup"),
            Err(e) => warn!(error = %e, "failed to reclaim empty scratch buffers"),
        }

        // Read-only consistency pass: surface backing files with no matching
        // row and rows whose content file vanished. Repair policy is a
        // separate ADR; for now this only logs (recovery #71).
        match ConsistencyChecker::new(&store).check() {
            Ok(report) => {
                if !report.orphan_files.is_empty() || !report.missing_files.is_empty() {
                    warn!(
                        orphan_files = report.orphan_files.len(),
                        missing_files = report.missing_files.len(),
                        "storage consistency check found discrepancies at startup"
                    );
                }
            }
            Err(e) => warn!(error = %e, "storage consistency check failed"),
        }

        // Reclaim the space left by superseded session snapshots before the
        // window shows: a VACUUM needs exclusive access to the database, which
        // only holds while nothing else is running yet.
        match store.run_maintenance() {
            Ok(outcome) if outcome.vacuumed => info!(
                before_bytes = outcome.before.file_bytes(),
                after_bytes = outcome.after.file_bytes(),
                "reclaimed database free space at startup"
            ),
            Ok(_) => {}
            Err(e) => warn!(error = %e, "database maintenance failed"),
        }

        let watcher_ignore = crate::watcher::handler::create_ignore_set();

        let authorized_paths = AuthorizedPaths::new();
        let hydrated = bless_persisted_sources(&store, &authorized_paths);
        if hydrated > 0 {
            info!(
                hydrated,
                "rehydrated blessed source paths from persisted buffers"
            );
        }

        let workspace_root = config
            .workspace
            .root
            .as_deref()
            .map(std::path::PathBuf::from)
            .and_then(|p| canonicalize_root(&p).ok())
            .filter(|p| p.is_dir());
        if let Some(root) = &workspace_root {
            info!(root = %root.display(), "workspace root restored from config");
        }

        let inbox_root = config
            .inbox
            .path
            .as_deref()
            .map(std::path::PathBuf::from)
            .and_then(|p| canonicalize_root(&p).ok())
            .filter(|p| p.is_dir());
        if let Some(root) = &inbox_root {
            info!(root = %root.display(), "inbox folder restored from config");
        }

        let workspace_index = Arc::new(RwLock::new(crate::workspace_index::WorkspaceIndex::new(
            workspace_root.clone(),
        )));

        let mut transforms = TransformRegistry::new();
        register_builtins(&mut transforms)?;
        info!(count = transforms.len(), "transform registry initialized");

        let mut preview_registry = ContentRendererRegistry::new();
        crate::preview::renderers::register_builtins(&mut preview_registry)
            .map_err(|e| format!("failed to register preview renderers: {e}"))?;
        info!(
            count = preview_registry.len(),
            "preview renderer registry initialized"
        );

        Ok(Self {
            store: Mutex::new(store),
            config_store,
            config: Mutex::new(config),
            writ_dir,
            buffers_dir,
            notes_root: RwLock::new(notes_root),
            notes_root_fallback: RwLock::new(notes_root_fallback),
            watcher_ignore,
            watcher: Mutex::new(None),
            notes_watcher: Mutex::new(None),
            pending_opens: Mutex::new(Vec::new()),
            frontend_ready: AtomicBool::new(false),
            transforms: RwLock::new(transforms),
            event_bus: Arc::new(EventBus::new()),
            update_phase: Mutex::new(UpdatePhase::default()),
            authorized_paths,
            preview_registry: Arc::new(RwLock::new(preview_registry)),
            preview_render_cache: Arc::new(RenderCache::new()),
            layout_state,
            notes_index,
            notes_index_cancel: Arc::new(AtomicBool::new(false)),
            quit: Arc::new(QuitState::new()),
            recovered_buffers: Mutex::new(recovered_buffers),
            was_dirty_shutdown,
            workspace_root: Mutex::new(workspace_root),
            workspace_watcher: Mutex::new(None),
            inbox_root: Mutex::new(inbox_root),
            inbox_watcher: Mutex::new(None),
            fts_scheduler: FtsScheduler::new(),
            workspace_index,
            search_generation: Arc::new(AtomicU64::new(0)),
            last_disk_hash: Mutex::new(recovered_disk_states),
        })
    }

    /// The notes folder as it stands now.
    ///
    /// Cloned rather than borrowed so no caller holds the lock across the work
    /// it does with the path.
    pub fn notes_root(&self) -> PathBuf {
        recover_poison(self.notes_root.read(), "state::notes_root").clone()
    }

    /// Points Writ at a different notes folder, after its files have been
    /// moved there.
    ///
    /// `root` is expected to be canonical: it is compared against canonical
    /// paths by [`Self::is_within_notes`], and a spelling that differs would
    /// lock every note in the folder out of saving.
    pub fn set_notes_root(&self, root: PathBuf) {
        let mut guard = recover_poison(self.notes_root.write(), "state::set_notes_root");
        *guard = root;
        drop(guard);
        self.clear_notes_root_fallback();
    }

    /// The folder the settings named that startup could not use, or `None`.
    pub fn notes_root_fallback(&self) -> Option<NotesRootFallback> {
        recover_poison(
            self.notes_root_fallback.read(),
            "state::notes_root_fallback",
        )
        .clone()
    }

    /// Forgets that folder, which a move to one Writ kept makes stale.
    fn clear_notes_root_fallback(&self) {
        let mut guard = recover_poison(
            self.notes_root_fallback.write(),
            "state::clear_notes_root_fallback",
        );
        *guard = None;
    }

    /// Returns `true` when `canonical_path` sits inside the open workspace
    /// folder. Used by the open-file origin gate: choosing a folder through
    /// the OS dialog expresses user intent for everything under it.
    pub fn is_within_workspace(&self, canonical_path: &str) -> bool {
        let guard = self
            .workspace_root
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .is_some_and(|root| std::path::Path::new(canonical_path).starts_with(root))
    }

    /// Returns `true` when `canonical_path` sits inside the watched inbox
    /// folder. Same origin-gate reasoning as [`Self::is_within_workspace`]:
    /// picking the inbox folder through the OS dialog expresses user intent
    /// for files under it (ADR-018).
    pub fn is_within_inbox(&self, canonical_path: &str) -> bool {
        let guard = self.inbox_root.lock().unwrap_or_else(|e| e.into_inner());
        guard
            .as_ref()
            .is_some_and(|root| std::path::Path::new(canonical_path).starts_with(root))
    }

    /// Returns `true` when `canonical_path` sits inside the notes folder.
    ///
    /// The notes folder is a blessed containment root: a note that arrives
    /// from a sync client and is opened from the sidebar has to save without a
    /// dialog, and exact set membership cannot express that because nothing
    /// blessed the path at an open site (ADR-028 §2). The root is canonical
    /// and so is the candidate, so the two agree on symlinks and, on Windows,
    /// on the UNC prefix.
    ///
    /// The argument is expected to be canonical already. A `..` component
    /// means it is not, and is refused rather than compared: `starts_with`
    /// matches component by component, so `<root>/../elsewhere.md` would
    /// otherwise read as contained on its way out of the folder.
    pub fn is_within_notes(&self, canonical_path: &str) -> bool {
        let path = std::path::Path::new(canonical_path);
        if path
            .components()
            .any(|component| component == std::path::Component::ParentDir)
        {
            return false;
        }
        path.starts_with(self.notes_root())
    }

    /// Records what a buffer's file held at the moment Writ read or wrote it,
    /// reading the rest of the record from the file's metadata.
    ///
    /// `size` is what the caller believes the file's length to be, used only
    /// when the metadata cannot be read.
    pub fn record_disk_state(
        &self,
        buffer_id: &str,
        path: &Path,
        digest: writ_core::hash::Sha256Digest,
        size: u64,
    ) {
        self.set_disk_state(buffer_id, disk_state_of(path, digest, size));
    }

    /// Records a state the caller already holds, which is what a save hands
    /// back: re-reading the file to learn what Writ just wrote to it would
    /// read every note twice per keystroke window, and would record whatever
    /// arrived in between as Writ's own.
    pub fn set_disk_state(&self, buffer_id: &str, state: DiskState) {
        let mut map = recover_poison(self.last_disk_hash.lock(), "state::set_disk_state");
        map.insert(buffer_id.to_string(), state);
    }

    /// [`Self::record_disk_state`] for a caller holding the file's bytes
    /// rather than an already-computed digest.
    pub fn record_disk_state_bytes(&self, buffer_id: &str, path: &Path, bytes: &[u8]) {
        self.record_disk_state(
            buffer_id,
            path,
            writ_core::hash::sha256_bytes(bytes),
            bytes.len() as u64,
        );
    }

    /// Drops the record for `buffer_id`, which the tab closing or the note
    /// being deleted is the end of.
    pub fn forget_disk_state(&self, buffer_id: &str) {
        let mut map = recover_poison(self.last_disk_hash.lock(), "state::forget_disk_state");
        map.remove(buffer_id);
    }

    /// What was last recorded for `buffer_id`, if anything.
    pub fn disk_state(&self, buffer_id: &str) -> Option<DiskState> {
        let map = recover_poison(self.last_disk_hash.lock(), "state::disk_state");
        map.get(buffer_id).copied()
    }

    /// `true` when `bytes` hashes to the digest last recorded for
    /// `buffer_id`.
    ///
    /// `false` for a buffer with no recorded digest — the honest answer for
    /// one Writ has not read or written this launch, where nothing rules out
    /// that the file changed underneath it.
    pub fn disk_hash_matches(&self, buffer_id: &str, bytes: &[u8]) -> bool {
        let map = recover_poison(self.last_disk_hash.lock(), "state::disk_hash_matches");
        map.get(buffer_id).map(|state| state.hash) == Some(writ_core::hash::sha256_bytes(bytes))
    }
}

/// Writes a note restored from the crash snapshot back into its file, minting
/// one first when the note never reached a file, and returns what that file
/// holds afterwards, when that was established.
///
/// The mint is the same policy the first keystroke uses
/// ([`crate::notes::attach_note_file`]), so a note recovered after a crash
/// carries the name it would have had if the crash had not happened.
///
/// The write goes through
/// [`restore_recovered_content`](BufferStore::restore_recovered_content),
/// which leaves a file that moved on while Writ was down exactly as it is and
/// writes the snapshot beside it. A relaunch is precisely when a sync client
/// has had time to deliver a newer version, and the snapshot is the older
/// text by definition.
///
/// A note whose file was never opened, because its bytes are not on this
/// machine, comes back with nothing: the first save reads it instead.
///
/// No stamp is passed: this runs while the app state is still being built, so
/// no watcher exists yet to mistake the write for somebody else's. The flags
/// come from the filesystem for the same reason — there is no test double in
/// the running app.
fn restore_recovered_buffer(
    store: &BufferStore,
    notes_root: &std::path::Path,
    recovered: &RecoveredBuffer,
) -> Result<Option<DiskState>, String> {
    let doc = store.get(&recovered.id).map_err(|e| e.to_string())?;
    if doc.source_path.is_none() {
        crate::notes::attach_note_file(
            store,
            notes_root,
            &recovered.id,
            &doc.title,
            chrono::Utc::now(),
        )?;
    }
    store
        .restore_recovered_content(&recovered.id, &recovered.content, None, None)
        .map(|outcome| outcome.disk_state())
        .map_err(|e| e.to_string())
}

/// Re-blesses the source paths of every persisted buffer, returning how many.
///
/// Blessing normally happens when a file is opened, in memory. Tabs outlive
/// the process, though, and a save writes the file the buffer came from, so a
/// restored tab has to carry its permission across the restart or the first
/// keystroke after a relaunch fails as an unauthorized write.
///
/// Both the stored path and its resolved form are recorded. The stored path is
/// what a save presents; resolving can fail outright for a file deleted while
/// Writ was closed, and refusing there would leave that tab permanently
/// unsavable instead of recreating the file it came from.
pub fn bless_persisted_sources(store: &BufferStore, authorized_paths: &AuthorizedPaths) -> usize {
    let mut hydrated = 0usize;
    for status in [
        writ_core::buffer::document::BufferStatus::Active,
        writ_core::buffer::document::BufferStatus::History,
    ] {
        let Ok(buffers) = store.list_by_status(status) else {
            continue;
        };
        for doc in buffers {
            let Some(source_path) = doc.source_path else {
                continue;
            };
            if let Ok(canonical) =
                canonicalize_for_authorization(std::path::Path::new(&source_path))
            {
                authorized_paths.record_blessed_source(canonical);
            }
            authorized_paths.record_blessed_source(source_path);
            hydrated += 1;
        }
    }
    hydrated
}

/// Resolves the notes folder, creates it, and returns it canonicalised.
///
/// A folder the user chose can fail: it points at a file, it sits on a volume
/// that is not mounted, it is written relative to a working directory that
/// means nothing to a launched app, the OS denies it. None of that is a reason
/// to refuse to launch, so each source is tried in turn — `WRIT_NOTES_DIR`,
/// then `config.notes.root`, then the default — and the first that can be
/// created wins. A bad environment variable therefore falls through to the
/// folder the user actually chose in Settings rather than skipping past it to
/// the default, which is the order the CLI resolves in
/// (`writ_cli::resolve_notes_dir`).
///
/// The value that failed comes back as the second element for the Settings
/// surface to show. Only the default failing is fatal, which is the existing
/// startup-failure path.
pub fn resolve_and_create_notes_root(
    sources: writ_core::notes::NotesRootSources<'_>,
    writ_dir: &Path,
) -> Result<(PathBuf, Option<NotesRootFallback>), Box<dyn std::error::Error>> {
    // Both spellings of the data folder. On macOS `writ_dir` is `/var/...`
    // where the resolved path is `/private/var/...`, so a candidate is
    // compared against the form it shares a prefix with: as spelled before it
    // exists, canonical once it does.
    let canonical_writ_dir = canonicalize_root(writ_dir).unwrap_or_else(|_| writ_dir.to_path_buf());
    let writ_dirs = [writ_dir, canonical_writ_dir.as_path()];
    let mut fallback: Option<NotesRootFallback> = None;

    for candidate in [sources.env_override, sources.configured] {
        let Some(chosen) = candidate.map(str::trim).filter(|value| !value.is_empty()) else {
            continue;
        };
        match usable_notes_root(chosen, sources.home, &writ_dirs) {
            Ok(root) => return Ok((root, fallback)),
            Err((reason, error)) => {
                warn!(
                    folder = chosen,
                    error, "notes folder unusable; trying the next one"
                );
                // The first source that failed is the one in effect, and the
                // one the Settings surface has to name.
                fallback.get_or_insert_with(|| NotesRootFallback {
                    from: chosen.to_string(),
                    reason,
                });
            }
        }
    }

    let default = writ_core::notes::resolve_notes_root_from(writ_core::notes::NotesRootSources {
        env_override: None,
        configured: None,
        ..sources
    })?;
    Ok((create_and_canonicalize(&default)?, fallback))
}

/// Resolves, creates and canonicalises one configured notes folder, or says
/// why it cannot be the one.
///
/// The data folder is asked about twice: of the path as spelled, before
/// anything is created, so a folder holding Writ's own data is never minted;
/// and of the canonical path once it exists, which is the comparison that is
/// true about folders rather than about spellings.
fn usable_notes_root(
    chosen: &str,
    home: Option<&Path>,
    writ_dirs: &[&Path],
) -> Result<PathBuf, (NotesRootFallbackReason, String)> {
    let unusable = |error: String| (NotesRootFallbackReason::Unusable, error);
    let holds = || {
        (
            NotesRootFallbackReason::HoldsWritData,
            "the folder holds Writ's own data".to_string(),
        )
    };

    let root = writ_core::notes::resolve_notes_root_from(writ_core::notes::NotesRootSources {
        env_override: Some(chosen),
        configured: None,
        data_dir: None,
        home,
    })
    .map_err(|e| unusable(e.to_string()))?;
    if holds_writ_data(&root, writ_dirs) {
        return Err(holds());
    }

    let root = create_and_canonicalize(&root).map_err(|e| unusable(e.to_string()))?;
    if holds_writ_data(&root, writ_dirs) {
        return Err(holds());
    }
    Ok(root)
}

/// Whether `root` may not be the notes folder because of where Writ's data is.
///
/// `root` is passed as both the candidate and the folder in force: startup has
/// no notes folder yet to be moved out of, so the only question
/// [`writ_core::notes::refuse_notes_root`] can answer here is the data-folder
/// one, and a candidate compared against itself asks exactly that. Every
/// spelling of the data folder is asked, because a candidate matches the
/// prefix of only the one it was written in.
fn holds_writ_data(root: &Path, writ_dirs: &[&Path]) -> bool {
    writ_dirs
        .iter()
        .any(|writ_dir| writ_core::notes::refuse_notes_root(root, root, writ_dir).is_some())
}

fn create_and_canonicalize(root: &std::path::Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(root)?;
    canonicalize_root(root)
}

/// Resolve the base directory for Writ's database, buffers, and config.
///
/// `WRIT_DATA_DIR` overrides the default so that several development
/// instances can run side by side without sharing one SQLite database.
/// When unset (or blank) the default is `<home>/.writ`.
///
/// The notes folder follows the same idea with its own variable:
/// `WRIT_NOTES_DIR` overrides it, and when only `WRIT_DATA_DIR` is set the
/// notes folder defaults to `<WRIT_DATA_DIR>/Writ` rather than `<home>/Writ`,
/// so an isolated instance never writes into the folder the user reads
/// (`writ_core::notes::resolve_notes_root_from`).
pub(crate) fn resolve_writ_dir(
    custom: Option<String>,
    home: Option<PathBuf>,
) -> Result<PathBuf, Box<dyn std::error::Error>> {
    if let Some(dir) = custom {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    Ok(home.ok_or("could not find home directory")?.join(".writ"))
}

#[cfg(test)]
mod tests {
    // `canonicalize_root`, not `std::fs::canonicalize`: on Windows the latter
    // returns a `\\?\` prefix that every path the resolver hands back has been
    // stripped of, and the two never compare equal.
    use super::{
        canonicalize_root, resolve_and_create_notes_root, resolve_writ_dir, NotesRootFallback,
        NotesRootFallbackReason,
    };
    use std::path::PathBuf;
    use tempfile::TempDir;
    use writ_core::notes::NotesRootSources;

    #[test]
    fn configured_notes_folder_is_created_and_used() {
        let home = TempDir::new().unwrap();
        let chosen = home.path().join("Documents").join("Notes");

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: None,
                configured: Some(&chosen.to_string_lossy()),
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert!(root.is_dir());
        assert_eq!(root, canonicalize_root(&chosen).unwrap());
        assert_eq!(fallback, None);
    }

    #[test]
    fn unusable_configured_notes_folder_falls_back_to_the_default() {
        let home = TempDir::new().unwrap();
        // A regular file where a folder would have to be: create_dir_all
        // cannot make a folder under it, on any platform.
        let blocked = home.path().join("not-a-folder");
        std::fs::write(&blocked, "x").unwrap();
        let chosen = blocked.join("Notes");

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: None,
                configured: Some(&chosen.to_string_lossy()),
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert!(root.is_dir());
        assert_eq!(
            root,
            canonicalize_root(&home.path().join("Writ")).unwrap()
        );
        assert_eq!(
            fallback,
            Some(NotesRootFallback {
                from: chosen.to_string_lossy().into_owned(),
                reason: NotesRootFallbackReason::Unusable,
            })
        );
    }

    #[test]
    fn a_relative_env_override_falls_through_to_the_configured_folder() {
        // A path relative to nothing in particular is what a shell profile that
        // exported `WRIT_NOTES_DIR=Writ` leaves behind, and a launched app has
        // no working directory worth anchoring it to. The folder the user chose
        // in Settings is a better answer than the default, and it is the answer
        // the CLI gives (`writ_cli::resolve_notes_dir`).
        let home = TempDir::new().unwrap();
        let chosen = home.path().join("Documents").join("Notes");

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: Some("Writ"),
                configured: Some(&chosen.to_string_lossy()),
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert_eq!(root, canonicalize_root(&chosen).unwrap());
        assert_eq!(
            fallback,
            Some(NotesRootFallback {
                from: "Writ".to_string(),
                reason: NotesRootFallbackReason::Unusable,
            }),
            "the value that was skipped is named for Settings"
        );
        assert!(
            !home.path().join("Writ").exists(),
            "the default was created"
        );
    }

    #[test]
    fn a_relative_env_override_alone_falls_through_to_the_default() {
        let home = TempDir::new().unwrap();

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: Some("Writ"),
                configured: None,
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert_eq!(
            root,
            canonicalize_root(&home.path().join("Writ")).unwrap()
        );
        assert_eq!(
            fallback,
            Some(NotesRootFallback {
                from: "Writ".to_string(),
                reason: NotesRootFallbackReason::Unusable,
            })
        );
    }

    #[test]
    fn unusable_default_notes_folder_is_a_startup_failure() {
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join("Writ"), "x").unwrap();

        let result = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: None,
                configured: None,
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        );

        assert!(result.is_err(), "nothing is left to fall back to");
    }

    fn writ_dir(home: &std::path::Path) -> PathBuf {
        home.join(".writ")
    }

    #[test]
    fn an_env_override_inside_the_data_folder_falls_back_to_the_default() {
        let home = TempDir::new().unwrap();
        let archive = writ_dir(home.path()).join("archive");
        std::fs::create_dir_all(&archive).unwrap();

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: Some(&archive.to_string_lossy()),
                configured: None,
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert_eq!(
            root,
            canonicalize_root(&home.path().join("Writ")).unwrap()
        );
        assert_eq!(
            fallback,
            Some(NotesRootFallback {
                from: archive.to_string_lossy().into_owned(),
                reason: NotesRootFallbackReason::HoldsWritData,
            })
        );
    }

    #[test]
    fn a_configured_folder_inside_the_data_folder_is_not_created() {
        let home = TempDir::new().unwrap();
        std::fs::create_dir_all(writ_dir(home.path())).unwrap();
        let chosen = writ_dir(home.path()).join("Notes");

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: None,
                configured: Some(&chosen.to_string_lossy()),
                data_dir: None,
                home: Some(home.path()),
            },
            &writ_dir(home.path()),
        )
        .unwrap();

        assert_eq!(
            root,
            canonicalize_root(&home.path().join("Writ")).unwrap()
        );
        assert_eq!(
            fallback.map(|fallback| fallback.reason),
            Some(NotesRootFallbackReason::HoldsWritData)
        );
        assert!(!chosen.exists(), "the folder was not created to be checked");
    }

    #[test]
    fn the_default_beside_the_database_is_kept_when_it_is_named_outright() {
        // The one folder inside the data folder that is allowed, reached
        // through the candidate branch rather than the default one: a dev
        // instance may name it in `WRIT_NOTES_DIR` instead of leaning on the
        // default (`writ_core::notes::refuse_notes_root`).
        let home = TempDir::new().unwrap();
        let data = home.path().join("dev-instance");
        std::fs::create_dir_all(&data).unwrap();
        let chosen = data.join("Writ");

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: Some(&chosen.to_string_lossy()),
                configured: None,
                data_dir: Some(&data),
                home: Some(home.path()),
            },
            &data,
        )
        .unwrap();

        assert_eq!(root, canonicalize_root(&chosen).unwrap());
        assert_eq!(fallback, None);
    }

    #[test]
    fn the_default_beside_the_database_is_kept_when_the_data_folder_is_overridden() {
        let home = TempDir::new().unwrap();
        let data = home.path().join("dev-instance");
        std::fs::create_dir_all(&data).unwrap();

        let (root, fallback) = resolve_and_create_notes_root(
            NotesRootSources {
                env_override: None,
                configured: None,
                data_dir: Some(&data),
                home: Some(home.path()),
            },
            &data,
        )
        .unwrap();

        assert_eq!(root, canonicalize_root(&data.join("Writ")).unwrap());
        assert_eq!(fallback, None);
    }

    #[test]
    fn defaults_to_home_dot_writ_when_unset() {
        let dir = resolve_writ_dir(None, Some(PathBuf::from("/home/user"))).unwrap();
        assert_eq!(dir, PathBuf::from("/home/user/.writ"));
    }

    #[test]
    fn blank_override_falls_back_to_home() {
        let dir = resolve_writ_dir(Some("  ".into()), Some(PathBuf::from("/home/user"))).unwrap();
        assert_eq!(dir, PathBuf::from("/home/user/.writ"));
    }

    #[test]
    fn honours_explicit_override() {
        let dir = resolve_writ_dir(
            Some("/tmp/writ-dev-1431".into()),
            Some(PathBuf::from("/home/user")),
        )
        .unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/writ-dev-1431"));
    }

    #[test]
    fn errors_when_no_home_and_no_override() {
        assert!(resolve_writ_dir(None, None).is_err());
    }
}
