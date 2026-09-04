use tempfile::TempDir;
use writ_core::config::{Accent, AppearanceConfig, Polarity, ProseFace, WritConfig};
use writ_storage::config_store::ConfigStore;

fn setup() -> (TempDir, ConfigStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let config_path = dir.path().join("config.toml");
    let store = ConfigStore::new(config_path);
    (dir, store)
}

#[test]
fn write_and_read_config() {
    let (_dir, store) = setup();
    let config = WritConfig::default();
    store.write(&config).expect("write failed");
    let read_back = store.read().expect("read failed");
    assert_eq!(read_back, config);
}

#[test]
fn read_missing_file_returns_default() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let store = ConfigStore::new(dir.path().join("nonexistent.toml"));
    let config = store.read().expect("read failed");
    assert_eq!(config, WritConfig::default());
}

#[test]
fn read_partial_config_fills_defaults() {
    let (_dir, store) = setup();
    std::fs::write(store.path(), "[editor]\nfont_size = 20\n").expect("write failed");
    let config = store.read().expect("read failed");
    assert_eq!(config.editor.font_size, 20);
    assert_eq!(
        config.editor.font_family,
        WritConfig::default().editor.font_family
    );
    assert_eq!(
        config.editor.word_wrap,
        WritConfig::default().editor.word_wrap
    );
    assert_eq!(
        config.editor.tab_size,
        WritConfig::default().editor.tab_size
    );
}

#[test]
fn appearance_and_status_bar_survive_a_disk_round_trip() {
    // get_config / set_config ride this same store, so what survives a write
    // and a read is what the frontend gets back after saving.
    let (_dir, store) = setup();
    let mut config = WritConfig::default();
    config.appearance.polarity = Polarity::Dark;
    config.appearance.accent = Accent::WritBlue;
    config.appearance.prose_face = ProseFace::Quattro;
    config.editor.status_bar = true;

    store.write(&config).expect("write failed");
    let read_back = store.read().expect("read failed");
    assert_eq!(read_back.appearance, config.appearance);
    assert!(read_back.editor.status_bar);
}

#[test]
fn a_config_written_before_adr_030_reads_back_with_the_new_defaults() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[editor]\nfont_size = 14\n\n[theme]\npreset = \"warp-dark\"\n",
    )
    .expect("write failed");
    let config = ConfigStore::new(path).read().expect("read failed");
    assert_eq!(config.editor.font_size, 14);
    assert_eq!(config.theme.preset, "warp-dark");
    assert!(!config.editor.status_bar);
    assert_eq!(config.appearance, AppearanceConfig::default());
}
