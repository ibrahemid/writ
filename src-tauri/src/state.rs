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

use crate::fts_scheduler::FtsScheduler;
use crate::poison::recover_poison;
use crate::preview::handler::RenderCache;
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

pub struct AppState {
    pub store: Mutex<BufferStore>,
    pub config_store: ConfigStore,
    pub config: Mutex<WritConfig>,
    pub writ_dir: PathBuf,
    pub buffers_dir: PathBuf,
    /// Canonical root of the notes folder. Always exists; created at startup.
    pub notes_root: PathBuf,
    /// The configured notes folder that could not be created, when startup
    /// fell back to the default one. `None` on every ordinary launch. The
    /// Settings surface reads this to tell the user which folder was refused
    /// and where the notes went instead.
    pub notes_root_fallback: Option<String>,
    pub watcher_ignore: IgnoreSet,
    pub watcher: Mutex<Option<WatcherHandle>>,
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
        let (notes_root, notes_root_fallback) =
            resolve_and_create_notes_root(writ_core::notes::NotesRootSources {
                env_override: notes_env_override.as_deref(),
                configured: config.notes.root.as_deref(),
                data_dir: data_dir_override.as_ref().map(|_| writ_dir.as_path()),
                home: home.as_deref(),
            })?;
        info!(path = %notes_root.display(), "notes folder ready");

        let store = BufferStore::new(conn, buffers_dir.clone());

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
                    Ok(state) => {
                        recovered_disk_states.insert(buf.id.clone(), state);
                    }
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

        // Heal any FTS drift left by a crash mid-save or a damaged index
        // (audit blocker #53.5). Runs after reclaim so deleted scratch rows
        // never count as drift.
        match store.verify_and_repair_fts() {
            Ok(false) => {}
            Ok(true) => info!("rebuilt drifted FTS index at startup"),
            Err(e) => warn!(error = %e, "failed to verify FTS index"),
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
            notes_root,
            notes_root_fallback,
            watcher_ignore,
            watcher: Mutex::new(None),
            pending_opens: Mutex::new(Vec::new()),
            frontend_ready: AtomicBool::new(false),
            transforms: RwLock::new(transforms),
            event_bus: Arc::new(EventBus::new()),
            update_phase: Mutex::new(UpdatePhase::default()),
            authorized_paths,
            preview_registry: Arc::new(RwLock::new(preview_registry)),
            preview_render_cache: Arc::new(RenderCache::new()),
            layout_state,
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
        path.starts_with(&self.notes_root)
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
/// holds afterwards.
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
/// No stamp is passed: this runs while the app state is still being built, so
/// no watcher exists yet to mistake the write for somebody else's.
fn restore_recovered_buffer(
    store: &BufferStore,
    notes_root: &std::path::Path,
    recovered: &RecoveredBuffer,
) -> Result<DiskState, String> {
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
        .restore_recovered_content(&recovered.id, &recovered.content, None)
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
/// that is not mounted, the OS denies it. None of that is a reason to refuse
/// to launch, so the failure is logged, the default folder is used instead,
/// and the value that failed comes back as the second element for the Settings
/// surface to show. Only the default failing is fatal, which is the existing
/// startup-failure path.
fn resolve_and_create_notes_root(
    sources: writ_core::notes::NotesRootSources<'_>,
) -> Result<(PathBuf, Option<String>), Box<dyn std::error::Error>> {
    let chosen = [sources.env_override, sources.configured]
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty());

    if let Some(chosen) = chosen {
        match writ_core::notes::resolve_notes_root_from(sources)
            .map_err(|e| e.to_string())
            .and_then(|root| create_and_canonicalize(&root).map_err(|e| e.to_string()))
        {
            Ok(root) => return Ok((root, None)),
            Err(error) => warn!(
                folder = chosen,
                error, "configured notes folder unusable; falling back to the default"
            ),
        }

        let default =
            writ_core::notes::resolve_notes_root_from(writ_core::notes::NotesRootSources {
                env_override: None,
                configured: None,
                ..sources
            })?;
        return Ok((create_and_canonicalize(&default)?, Some(chosen.to_string())));
    }

    let default = writ_core::notes::resolve_notes_root_from(sources)?;
    Ok((create_and_canonicalize(&default)?, None))
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
    use super::{resolve_and_create_notes_root, resolve_writ_dir};
    use std::path::PathBuf;
    use tempfile::TempDir;
    use writ_core::notes::NotesRootSources;

    #[test]
    fn configured_notes_folder_is_created_and_used() {
        let home = TempDir::new().unwrap();
        let chosen = home.path().join("Documents").join("Notes");

        let (root, fallback) = resolve_and_create_notes_root(NotesRootSources {
            env_override: None,
            configured: Some(&chosen.to_string_lossy()),
            data_dir: None,
            home: Some(home.path()),
        })
        .unwrap();

        assert!(root.is_dir());
        assert_eq!(root, std::fs::canonicalize(&chosen).unwrap());
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

        let (root, fallback) = resolve_and_create_notes_root(NotesRootSources {
            env_override: None,
            configured: Some(&chosen.to_string_lossy()),
            data_dir: None,
            home: Some(home.path()),
        })
        .unwrap();

        assert!(root.is_dir());
        assert_eq!(
            root,
            std::fs::canonicalize(home.path().join("Writ")).unwrap()
        );
        assert_eq!(fallback.as_deref(), Some(chosen.to_string_lossy().as_ref()));
    }

    #[test]
    fn unusable_default_notes_folder_is_a_startup_failure() {
        let home = TempDir::new().unwrap();
        std::fs::write(home.path().join("Writ"), "x").unwrap();

        let result = resolve_and_create_notes_root(NotesRootSources {
            env_override: None,
            configured: None,
            data_dir: None,
            home: Some(home.path()),
        });

        assert!(result.is_err(), "nothing is left to fall back to");
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
