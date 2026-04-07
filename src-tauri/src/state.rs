use std::path::PathBuf;
use std::sync::Mutex;
use tracing::info;
use writ_core::config::WritConfig;
use writ_storage::buffer_store::BufferStore;
use writ_storage::config_store::ConfigStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

use crate::watcher::handler::IgnoreSet;

pub struct AppState {
    pub store: Mutex<BufferStore>,
    pub config_store: ConfigStore,
    pub config: Mutex<WritConfig>,
    pub writ_dir: PathBuf,
    pub buffers_dir: PathBuf,
    pub watcher_ignore: IgnoreSet,
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
        let watcher_ignore = crate::watcher::handler::create_ignore_set();

        Ok(Self {
            store: Mutex::new(store),
            config_store,
            config: Mutex::new(config),
            writ_dir,
            buffers_dir,
            watcher_ignore,
        })
    }
}
