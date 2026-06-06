use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, RwLock};
use tracing::{info, warn};
use writ_core::config::WritConfig;
use writ_core::events::bus::EventBus;
use writ_core::preview::ContentRendererRegistry;
use writ_core::update::UpdatePhase;
use writ_plugin::transform::builtins::register_builtins;
use writ_plugin::transform::TransformRegistry;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

use writ_storage::layout_state::LayoutStateStore;

use crate::preview::handler::RenderCache;
use crate::security::{canonicalize_for_authorization, AuthorizedPaths};
use crate::watcher::handler::{IgnoreSet, WatcherHandle};

pub struct AppState {
    pub store: Mutex<BufferStore>,
    pub config_store: ConfigStore,
    pub config: Mutex<WritConfig>,
    pub writ_dir: PathBuf,
    pub buffers_dir: PathBuf,
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
}

impl AppState {
    pub fn initialize() -> Result<Self, Box<dyn std::error::Error>> {
        let writ_dir = resolve_writ_dir(std::env::var("WRIT_DATA_DIR").ok(), dirs::home_dir())?;

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

        let store = BufferStore::new(conn, buffers_dir.clone());
        match store.reclaim_empty_scratch() {
            Ok(0) => {}
            Ok(count) => info!(count, "reclaimed empty scratch buffers at startup"),
            Err(e) => warn!(error = %e, "failed to reclaim empty scratch buffers"),
        }
        let watcher_ignore = crate::watcher::handler::create_ignore_set();

        let authorized_paths = AuthorizedPaths::new();
        let mut hydrated = 0usize;
        for status in [
            writ_core::buffer::document::BufferStatus::Active,
            writ_core::buffer::document::BufferStatus::History,
        ] {
            if let Ok(buffers) = store.list_by_status(status) {
                for doc in buffers {
                    if let Some(source_path) = doc.source_path.as_deref() {
                        if let Ok(canonical) =
                            canonicalize_for_authorization(std::path::Path::new(source_path))
                        {
                            authorized_paths.record_blessed_source(canonical);
                            hydrated += 1;
                        }
                    }
                }
            }
        }
        if hydrated > 0 {
            info!(
                hydrated,
                "rehydrated blessed source paths from persisted buffers"
            );
        }

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
        })
    }
}

/// Resolve the base directory for Writ's database, buffers, and config.
///
/// `WRIT_DATA_DIR` overrides the default so that several development
/// instances can run side by side without sharing one SQLite database.
/// When unset (or blank) the default is `<home>/.writ`.
fn resolve_writ_dir(
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
    use super::resolve_writ_dir;
    use std::path::PathBuf;

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
        let dir = resolve_writ_dir(Some("/tmp/writ-dev-1431".into()), Some(PathBuf::from("/home/user")))
            .unwrap();
        assert_eq!(dir, PathBuf::from("/tmp/writ-dev-1431"));
    }

    #[test]
    fn errors_when_no_home_and_no_override() {
        assert!(resolve_writ_dir(None, None).is_err());
    }
}
