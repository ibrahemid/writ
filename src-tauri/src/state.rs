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

use crate::preview::webview_manager::PreviewWebviewManager;
use crate::preview::window_manager::WindowManager;
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
    pub preview_webviews: Arc<PreviewWebviewManager>,
    pub window_manager: Arc<WindowManager>,
}

impl AppState {
    pub fn initialize() -> Result<Self, Box<dyn std::error::Error>> {
        let writ_dir = dirs::home_dir()
            .ok_or("could not find home directory")?
            .join(".writ");

        std::fs::create_dir_all(&writ_dir)?;

        let buffers_dir = writ_dir.join("buffers");
        std::fs::create_dir_all(&buffers_dir)?;

        let db_path = writ_dir.join("writ.db");
        let conn = open_database(&db_path)?;
        run_migrations(&conn)?;
        info!(path = %db_path.display(), "database initialized");

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
            preview_webviews: PreviewWebviewManager::new(),
            window_manager: Arc::new(WindowManager::with_main()),
        })
    }
}
