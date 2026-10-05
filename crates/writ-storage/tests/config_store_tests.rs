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
    // Off is the non-default now, so writing it is what proves the field
    // rides the round trip rather than falling back to the default.
    config.editor.status_bar = false;

    store.write(&config).expect("write failed");
    let read_back = store.read().expect("read failed");
    assert_eq!(read_back.appearance, config.appearance);
    assert!(!read_back.editor.status_bar);
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
    assert!(config.editor.status_bar);
    assert_eq!(config.appearance, AppearanceConfig::default());
}

/// A config past a mebibyte, so a write takes long enough for a reader to land
/// inside it. The padding is a TOML comment, so the bytes still parse as the
/// settings they carry.
fn padded_config(store: &ConfigStore, font_size: u32, fill: char) -> String {
    let mut config = WritConfig::default();
    config.editor.font_size = font_size;
    let mut text = store.serialize(&config).expect("serialize");
    text.push_str("# ");
    text.extend(std::iter::repeat_n(fill, 1 << 20));
    text.push('\n');
    let parsed = WritConfig::parse(&text).expect("a padded config parses");
    assert_eq!(parsed.editor.font_size, font_size);
    text
}

#[test]
fn a_reader_racing_config_writes_sees_one_whole_file_or_the_other() {
    // A reader that lands between a truncate and the write that follows it
    // sees what a crash at that moment leaves behind: nothing, or the front
    // of the new bytes.
    const WRITES: usize = 60;
    let (_dir, store) = setup();
    let first = padded_config(&store, 14, 'a');
    let second = padded_config(&store, 20, 'b');
    store.write_serialized(&first).expect("seed the config");

    let writing = std::sync::atomic::AtomicBool::new(true);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let (mut reads, mut saw_first, mut saw_second) = (0usize, false, false);
            while writing.load(std::sync::atomic::Ordering::Acquire) {
                let bytes =
                    std::fs::read(store.path()).map_err(|e| format!("read {reads} failed: {e}"))?;
                if bytes == first.as_bytes() {
                    saw_first = true;
                } else if bytes == second.as_bytes() {
                    saw_second = true;
                } else {
                    return Err(format!(
                        "read {reads} held {} bytes, neither config ({} bytes)",
                        bytes.len(),
                        first.len()
                    ));
                }
                reads += 1;
                // Windows will not rename over a file another handle holds,
                // so the reader lets go between reads or the writer's retries
                // run out.
                std::thread::sleep(std::time::Duration::from_micros(200));
            }
            Ok((reads, saw_first, saw_second))
        });

        for round in 0..WRITES {
            let text = if round % 2 == 0 { &second } else { &first };
            store.write_serialized(text).expect("config write");
        }
        writing.store(false, std::sync::atomic::Ordering::Release);

        let (reads, saw_first, saw_second) = reader
            .join()
            .expect("the reader thread")
            .unwrap_or_else(|torn| panic!("{torn}"));
        assert!(
            saw_first && saw_second,
            "the reader has to have raced the writes: {reads} reads, \
             first seen {saw_first}, second seen {saw_second}"
        );
    });
}

#[cfg(unix)]
#[test]
fn a_symlinked_config_stays_a_link_and_its_target_takes_the_write() {
    let dir = TempDir::new().expect("temp dir");
    let dotfiles = dir.path().join("dotfiles");
    std::fs::create_dir(&dotfiles).expect("dotfiles folder");
    let target = dotfiles.join("writ.toml");
    std::fs::write(&target, "[editor]\nfont_size = 14\n").expect("seed the target");
    let link = dir.path().join("config.toml");
    std::os::unix::fs::symlink(&target, &link).expect("link config.toml");

    let store = ConfigStore::new(link.clone());
    let written = "[editor]\nfont_size = 20\n";
    store.write_serialized(written).expect("config write");

    let entry = std::fs::symlink_metadata(&link).expect("config.toml is there");
    assert!(
        entry.file_type().is_symlink(),
        "config.toml is still a link"
    );
    assert_eq!(std::fs::read_link(&link).expect("read the link"), target);
    assert_eq!(
        std::fs::read_to_string(&target).expect("read the target"),
        written
    );
    assert_eq!(
        store
            .read()
            .expect("read through the link")
            .editor
            .font_size,
        20
    );
}

#[cfg(unix)]
#[test]
fn a_config_linked_to_a_file_not_written_yet_is_created_at_the_link_s_target() {
    let dir = TempDir::new().expect("temp dir");
    let writ_dir = dir.path().join("writ");
    std::fs::create_dir(&writ_dir).expect("writ folder");
    let link = writ_dir.join("config.toml");
    std::os::unix::fs::symlink("../dotfiles/writ.toml", &link).expect("link config.toml");

    let store = ConfigStore::new(link.clone());
    let written = "[editor]\nfont_size = 20\n";
    store.write_serialized(written).expect("config write");

    let entry = std::fs::symlink_metadata(&link).expect("config.toml is there");
    assert!(
        entry.file_type().is_symlink(),
        "config.toml is still a link"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("dotfiles").join("writ.toml"))
            .expect("the link's target was created"),
        written
    );
}

#[cfg(unix)]
#[test]
fn a_config_with_a_second_name_is_refused_and_both_names_keep_their_bytes() {
    use writ_storage::atomic::AtomicWriteError;
    use writ_storage::errors::StorageError;

    let (dir, store) = setup();
    let before = "[editor]\nfont_size = 14\n";
    std::fs::write(store.path(), before).expect("seed the config");
    let second_name = dir.path().join("config-backup.toml");
    std::fs::hard_link(store.path(), &second_name).expect("hard link");

    let refused = store.write_serialized("[editor]\nfont_size = 20\n");

    assert!(
        matches!(
            refused,
            Err(StorageError::AtomicWrite(AtomicWriteError::HardLinked {
                links: 2
            }))
        ),
        "a hard-linked config is refused, got {refused:?}"
    );
    for name in [store.path(), second_name.as_path()] {
        assert_eq!(
            std::fs::read_to_string(name).expect("read"),
            before,
            "{} kept its bytes",
            name.display()
        );
    }
}
