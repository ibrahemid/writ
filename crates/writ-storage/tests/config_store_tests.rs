use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

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

/// Stops the racing reader when the writer leaves its loop, whether it ran
/// every write or a failed write panicked, so `thread::scope` is never left
/// waiting on a reader that is still looping.
struct StopReader<'a>(&'a AtomicBool);

impl Drop for StopReader<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

#[test]
fn a_reader_racing_config_writes_sees_one_whole_file_or_the_other() {
    // A reader that lands between a truncate and the write that follows it
    // sees what a crash at that moment leaves behind: nothing, or the front
    // of the new bytes.
    const WRITES: usize = 60;
    const PAUSE: Duration = Duration::from_micros(200);
    let (_dir, store) = setup();
    let first = padded_config(&store, 14, 'a');
    let second = padded_config(&store, 20, 'b');
    store.write_serialized(&first).expect("seed the config");

    let writing = AtomicBool::new(true);
    std::thread::scope(|scope| {
        let reader = scope.spawn(|| {
            let (mut reads, mut saw_first, mut saw_second) = (0usize, false, false);
            while writing.load(Ordering::Acquire) {
                let started = Instant::now();
                let bytes =
                    std::fs::read(store.path()).map_err(|e| format!("read {reads} failed: {e}"))?;
                let read_took = started.elapsed();
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
                // The reader holds the file for the whole of each read, and
                // Windows can refuse a rename over a file another handle holds
                // open. write_atomic retries for about 200 ms, and a short
                // pause does not stop all ten attempts landing on reads, so on
                // Windows the pause is at least nine reads long and the reader
                // holds the file a tenth of the time at most. Unix renames over
                // an open file; its pause stays short so that reads keep
                // landing inside the writes.
                let pause = if cfg!(windows) {
                    PAUSE.max(read_took * 9)
                } else {
                    PAUSE
                };
                std::thread::sleep(pause);
            }
            Ok((reads, saw_first, saw_second))
        });

        let stop_reader = StopReader(&writing);
        for round in 0..WRITES {
            let text = if round % 2 == 0 { &second } else { &first };
            store.write_serialized(text).expect("config write");
        }
        drop(stop_reader);

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

/// The config file the killed writer writes over. Set only for that child.
const WRITER_CONFIG_VAR: &str = "WRIT_TEST_KILLED_WRITER_CONFIG";

/// The files holding the payloads the killed writer alternates between, joined
/// as a path list. The parent writes them, so the child writes exactly the
/// bytes the parent checks for.
const WRITER_PAYLOADS_VAR: &str = "WRIT_TEST_KILLED_WRITER_PAYLOADS";

/// The child half of the killed-writer test, run by name through this test
/// binary's own filter.
const WRITER_TEST: &str = "a_config_writer_writes_until_it_is_killed";

/// Writers killed per run. Only a kill that lands after an in-place write has
/// truncated the file and before its last byte catches the tear, which on macOS
/// is about two kills in five, so sixteen let a broken store through fewer than
/// one run in a thousand.
const KILL_ROUNDS: usize = 16;

/// The longest any wait in the killed-writer test lasts before it fails.
const WAIT_LIMIT: Duration = Duration::from_secs(30);

/// How often the killed-writer test looks at the folder and the writer.
const POLL: Duration = Duration::from_millis(1);

/// How long a writer that nothing kills keeps going, so one left behind by a
/// test process that died stops on its own.
const WRITER_LIFETIME: Duration = Duration::from_secs(60);

#[test]
#[ignore = "the writer half of a_config_write_killed_part_way_leaves_a_whole_config_behind"]
fn a_config_writer_writes_until_it_is_killed() {
    let (Some(config), Some(payloads)) = (
        std::env::var_os(WRITER_CONFIG_VAR),
        std::env::var_os(WRITER_PAYLOADS_VAR),
    ) else {
        return;
    };
    let payloads: Vec<String> = std::env::split_paths(&payloads)
        .map(|path| std::fs::read_to_string(path).expect("read a payload"))
        .collect();
    let store = ConfigStore::new(PathBuf::from(config));
    let stop_at = Instant::now() + WRITER_LIFETIME;
    for text in payloads.iter().cycle() {
        if Instant::now() >= stop_at {
            return;
        }
        store.write_serialized(text).expect("config write");
    }
}

/// The writer process, killed and reaped however the test leaves it, so a
/// failed assertion does not leave it writing.
struct Writer(Child);

impl Writer {
    /// Waits up to [`WAIT_LIMIT`] for the writer to exit.
    fn reap(&mut self) -> Option<ExitStatus> {
        let deadline = Instant::now() + WAIT_LIMIT;
        loop {
            match self.0.try_wait() {
                Ok(Some(status)) => return Some(status),
                Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL),
                _ => return None,
            }
        }
    }

    /// What the writer printed to stderr. Read only once it has exited, when
    /// the pipe is sure to end.
    fn stderr(&mut self) -> String {
        let mut text = String::new();
        if let Some(mut pipe) = self.0.stderr.take() {
            let _ = pipe.read_to_string(&mut text);
        }
        text
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        let _ = self.0.kill();
        self.reap();
    }
}

/// Whether a write to `config` is visibly under way: its size is no longer the
/// seeded config's, which is what an in-place write that has truncated it
/// shows, or a file beside it has started filling, which is the temp file an
/// atomic write renames over it.
fn write_in_flight(folder: &Path, config: &Path, seeded_len: usize) -> bool {
    let size = |path: &Path| std::fs::metadata(path).map(|entry| entry.len()).ok();
    if size(config).is_some_and(|len| len != seeded_len as u64) {
        return true;
    }
    std::fs::read_dir(folder)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .any(|path| path != config && size(&path).is_some_and(|len| len > 0))
}

/// One round: seed `previous` in a folder of its own, start a second process
/// writing the payloads listed in `payload_list` over it, kill that process
/// once a write is in flight, and read what is left.
fn kill_a_config_writer_part_way(
    round: usize,
    previous: &str,
    payloads: &[String],
    payload_list: &std::ffi::OsStr,
) {
    // A killed atomic write leaves its temp file behind, which would read as a
    // write in flight before the next round's writer had started.
    let (dir, store) = setup();
    store.write_serialized(previous).expect("seed the config");

    let mut writer = Writer(
        Command::new(std::env::current_exe().expect("the test binary"))
            .args(["--exact", WRITER_TEST, "--ignored", "--nocapture"])
            .env(WRITER_CONFIG_VAR, store.path())
            .env(WRITER_PAYLOADS_VAR, payload_list)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the writer"),
    );

    let deadline = Instant::now() + WAIT_LIMIT;
    while !write_in_flight(dir.path(), store.path(), previous.len()) {
        if let Some(status) = writer.0.try_wait().expect("poll the writer") {
            panic!(
                "round {round}: the writer stopped before a write was seen ({status}): {}",
                writer.stderr()
            );
        }
        assert!(
            Instant::now() < deadline,
            "round {round}: no write was seen in flight within {WAIT_LIMIT:?}"
        );
        std::thread::sleep(POLL);
    }
    writer.0.kill().expect("kill the writer");
    assert!(
        writer.reap().is_some(),
        "round {round}: the writer was still running {WAIT_LIMIT:?} after it was killed"
    );

    let bytes = std::fs::read(store.path()).expect("the config is still there");
    assert!(
        bytes == previous.as_bytes() || payloads.iter().any(|text| bytes == text.as_bytes()),
        "round {round}: the config held {} bytes after the kill, neither the previous \
         config ({} bytes) nor a whole payload ({} bytes)",
        bytes.len(),
        previous.len(),
        payloads[0].len()
    );
    let text = std::str::from_utf8(&bytes).expect("the config is text");
    WritConfig::parse(text).expect("the config parses");
}

#[test]
fn a_config_write_killed_part_way_leaves_a_whole_config_behind() {
    // A write interrupted after truncation leaves the previous file readable.
    // The writer is a separate process killed while a write is in flight, so
    // nothing it would have done next can repair the file.
    let (_dir, store) = setup();
    // One byte shorter than either payload, so a payload renamed over it also
    // changes its size.
    let previous = padded_config(&store, 9, 'a');
    let payloads = [
        padded_config(&store, 20, 'b'),
        padded_config(&store, 22, 'c'),
    ];
    // Outside every config's folder, where a growing file reads as a write.
    let payload_dir = TempDir::new().expect("payload folder");
    let payload_paths: Vec<PathBuf> = payloads
        .iter()
        .enumerate()
        .map(|(index, text)| {
            let path = payload_dir.path().join(format!("payload-{index}.toml"));
            std::fs::write(&path, text).expect("write a payload");
            path
        })
        .collect();
    let payload_list = std::env::join_paths(&payload_paths).expect("join the payload paths");

    for round in 0..KILL_ROUNDS {
        kill_a_config_writer_part_way(round, &previous, &payloads, &payload_list);
    }
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
fn a_config_linked_to_a_file_not_written_yet_creates_the_link_s_target_and_its_folder() {
    let dir = TempDir::new().expect("temp dir");
    let writ_dir = dir.path().join("writ");
    std::fs::create_dir(&writ_dir).expect("writ folder");
    let link = writ_dir.join("config.toml");
    std::os::unix::fs::symlink("../dotfiles/writ.toml", &link).expect("link config.toml");
    assert!(!dir.path().join("dotfiles").exists());

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

/// Asserts that `link` is still a link and still points at `points_at`.
#[cfg(unix)]
fn assert_still_links_to(link: &Path, points_at: &str) {
    let entry = std::fs::symlink_metadata(link).expect("the link is there");
    assert!(
        entry.file_type().is_symlink(),
        "{} is still a link",
        link.display()
    );
    assert_eq!(
        std::fs::read_link(link).expect("read the link"),
        Path::new(points_at),
        "{} still points where it did",
        link.display()
    );
}

#[cfg(unix)]
#[test]
fn a_config_at_the_head_of_a_chain_of_links_to_nothing_is_created_at_the_chain_s_end() {
    let dir = TempDir::new().expect("temp dir");
    std::fs::create_dir(dir.path().join("dotfiles")).expect("dotfiles folder");
    let link = dir.path().join("config.toml");
    let middle = dir.path().join("link2");
    std::os::unix::fs::symlink("link2", &link).expect("link config.toml");
    std::os::unix::fs::symlink("dotfiles/writ.toml", &middle).expect("link link2");

    let written = "[editor]\nfont_size = 20\n";
    ConfigStore::new(link.clone())
        .write_serialized(written)
        .expect("config write");

    assert_still_links_to(&link, "link2");
    assert_still_links_to(&middle, "dotfiles/writ.toml");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("dotfiles").join("writ.toml"))
            .expect("the end of the chain was created"),
        written
    );
}

#[cfg(unix)]
#[test]
fn a_config_in_a_loop_of_links_is_refused_and_every_link_is_kept() {
    use writ_storage::errors::StorageError;

    let dir = TempDir::new().expect("temp dir");
    let link = dir.path().join("config.toml");
    let other = dir.path().join("other.toml");
    std::os::unix::fs::symlink("other.toml", &link).expect("link config.toml");
    std::os::unix::fs::symlink("config.toml", &other).expect("link other.toml");

    let refused = ConfigStore::new(link.clone()).write_serialized("[editor]\nfont_size = 20\n");

    assert!(
        matches!(
            &refused,
            Err(StorageError::Io(e)) if e.kind() == std::io::ErrorKind::InvalidInput
        ),
        "a loop of links is refused, got {refused:?}"
    );
    assert_still_links_to(&link, "other.toml");
    assert_still_links_to(&other, "config.toml");
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list the folder")
        .map(|entry| entry.expect("folder entry").file_name())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["config.toml", "other.toml"],
        "nothing else was written"
    );
}

#[cfg(unix)]
#[test]
fn a_config_in_a_folder_reached_through_a_link_follows_its_dot_dot_from_the_real_folder() {
    let dir = TempDir::new().expect("temp dir");
    let real_folder = dir.path().join("dotfiles").join("writ");
    std::fs::create_dir_all(&real_folder).expect("real config folder");
    let shared = dir.path().join("dotfiles").join("shared");
    std::fs::create_dir(&shared).expect("shared folder");
    std::fs::write(shared.join("writ.toml"), "[editor]\nfont_size = 14\n")
        .expect("seed the link's target");
    let config_link = real_folder.join("config.toml");
    std::os::unix::fs::symlink("../shared/writ.toml", &config_link).expect("link config.toml");
    let home = dir.path().join("home");
    std::fs::create_dir(&home).expect("home folder");
    let folder_link = home.join(".writ");
    std::os::unix::fs::symlink("../dotfiles/writ", &folder_link).expect("link the folder");

    let store = ConfigStore::new(folder_link.join("config.toml"));
    let written = "[editor]\nfont_size = 20\n";
    store.write_serialized(written).expect("config write");

    assert_still_links_to(&folder_link, "../dotfiles/writ");
    assert_still_links_to(&config_link, "../shared/writ.toml");
    assert_eq!(
        std::fs::read_to_string(shared.join("writ.toml")).expect("read the link's target"),
        written
    );
    assert!(
        !home.join("shared").exists(),
        "nothing was written beside the folder link"
    );
    assert_eq!(
        store
            .read()
            .expect("read through the links")
            .editor
            .font_size,
        20
    );
}

/// Links `config.toml -> link2 -> ... -> link{links} -> writ.toml` in
/// `folder`, `links` links in all, and returns each link with what it points
/// at.
#[cfg(unix)]
fn chain_of_links(folder: &Path, links: usize) -> Vec<(PathBuf, String)> {
    (1..=links)
        .map(|n| {
            let link = if n == 1 {
                folder.join("config.toml")
            } else {
                folder.join(format!("link{n}"))
            };
            let points_at = if n == links {
                "writ.toml".to_owned()
            } else {
                format!("link{}", n + 1)
            };
            std::os::unix::fs::symlink(&points_at, &link).expect("link the chain");
            (link, points_at)
        })
        .collect()
}

#[cfg(unix)]
#[test]
fn a_config_at_the_head_of_32_links_is_written_at_the_chain_s_end() {
    let dir = TempDir::new().expect("temp dir");
    // macOS keeps temp folders under /var, itself a link that a read through
    // the chain would count toward the 32.
    let folder = std::fs::canonicalize(dir.path()).expect("resolve the temp dir");
    std::fs::write(folder.join("writ.toml"), "[editor]\nfont_size = 14\n")
        .expect("seed the end of the chain");
    let chain = chain_of_links(&folder, 32);

    let store = ConfigStore::new(folder.join("config.toml"));
    let written = "[editor]\nfont_size = 20\n";
    store.write_serialized(written).expect("config write");

    for (link, points_at) in &chain {
        assert_still_links_to(link, points_at);
    }
    assert_eq!(
        std::fs::read_to_string(folder.join("writ.toml")).expect("read the end of the chain"),
        written
    );
    assert_eq!(
        store
            .read()
            .expect("read through the chain")
            .editor
            .font_size,
        20
    );
}

#[cfg(unix)]
#[test]
fn a_config_at_the_head_of_33_links_is_refused_and_nothing_is_replaced() {
    use writ_storage::errors::StorageError;

    let dir = TempDir::new().expect("temp dir");
    let before = "[editor]\nfont_size = 14\n";
    std::fs::write(dir.path().join("writ.toml"), before).expect("seed the end of the chain");
    let chain = chain_of_links(dir.path(), 33);

    let refused = ConfigStore::new(dir.path().join("config.toml"))
        .write_serialized("[editor]\nfont_size = 20\n");

    assert!(
        matches!(
            &refused,
            Err(StorageError::Io(e)) if e.kind() == std::io::ErrorKind::InvalidInput
        ),
        "a chain of 33 links is refused, got {refused:?}"
    );
    for (link, points_at) in &chain {
        assert_still_links_to(link, points_at);
    }
    assert_eq!(
        std::fs::read_to_string(dir.path().join("writ.toml")).expect("read the end of the chain"),
        before,
        "the end of the chain kept its bytes"
    );
    let mut names: Vec<_> = std::fs::read_dir(dir.path())
        .expect("list the folder")
        .map(|entry| entry.expect("folder entry").file_name())
        .collect();
    names.sort();
    let mut expected: Vec<_> = chain
        .iter()
        .map(|(link, _)| link.file_name().expect("link name").to_owned())
        .chain([std::ffi::OsString::from("writ.toml")])
        .collect();
    expected.sort();
    assert_eq!(names, expected, "nothing else was written");
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
