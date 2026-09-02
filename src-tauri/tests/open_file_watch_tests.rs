//! The open-file watcher against a real filesystem.
//!
//! The unit tests in `watcher::open_files` prove the registry's bookkeeping
//! and the classifier's decisions with injected backends. These run the
//! platform's own watcher over a real folder, because the one thing an
//! injected backend cannot prove is that a temp-plus-rename over a watched
//! file reaches Writ at all — which is the write every careful program makes,
//! and the reason the folder is watched rather than the file.

use std::path::Path;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use writ_core::events::bus::{EventBus, WritEvent};
use writ_core::watcher::ignore::DEFAULT_IGNORE_TTL;
use writ_tauri_lib::security::resolve_for_containment;
use writ_tauri_lib::watcher::handler::{create_ignore_set, start_notes_watcher};
use writ_tauri_lib::watcher::open_files::{start_open_file_watcher, WatchOutcome, WatcherKind};

/// Long enough for the 500 ms debounce plus the platform's own notification
/// latency, and for the ignore TTL to be nowhere near expiry.
const SETTLE: Duration = Duration::from_secs(3);

/// The path both the ignore stamps and the watcher registry key on. The
/// handler's own `ignore_key_path` is crate-private; this is the same
/// resolution, reached through the function it delegates to.
fn resolved(path: &Path) -> String {
    resolve_for_containment(path).unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// The notes folder as the app holds it: canonical, the way startup stores it
/// and the way the platform's watcher reports paths under it. A watcher rooted
/// at an uncanonical path drops every event, because the containment check
/// misses.
fn canonical(dir: &Path) -> std::path::PathBuf {
    std::path::PathBuf::from(resolved(dir))
}

/// Writes `bytes` to `path` the way a careful program does: into a sibling
/// temp file, then renamed over the target. The rename gives the file a new
/// inode, which is what kills a watch bound to the file rather than the
/// folder.
fn write_by_temp_and_rename(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("writ-test-tmp");
    std::fs::write(&temp, bytes).expect("write temp");
    std::fs::rename(&temp, path).expect("rename over target");
}

/// Every `BufferExternal` the bus carried within `SETTLE`.
fn collect_external(rx: &mpsc::Receiver<WritEvent>) -> Vec<WritEvent> {
    let deadline = Instant::now() + SETTLE;
    let mut seen = Vec::new();
    while let Some(left) = deadline.checked_duration_since(Instant::now()) {
        match rx.recv_timeout(left) {
            Ok(event @ WritEvent::BufferExternal { .. }) => seen.push(event),
            Ok(_) => {}
            Err(_) => break,
        }
    }
    seen
}

fn bus_with_channel() -> (Arc<EventBus>, mpsc::Receiver<WritEvent>) {
    let bus = Arc::new(EventBus::new());
    let (tx, rx) = mpsc::channel();
    bus.subscribe(move |event| {
        let _ = tx.send(event.clone());
    });
    (bus, rx)
}

#[test]
fn a_file_opened_from_anywhere_reports_one_change_when_another_program_rewrites_it() {
    let notes = TempDir::new().expect("notes dir");
    let elsewhere = TempDir::new().expect("some other folder");
    let file = elsewhere.path().join("shared.md");
    std::fs::write(&file, b"as another program left it\n").expect("seed file");

    let (bus, rx) = bus_with_channel();
    let watcher = start_open_file_watcher(bus, create_ignore_set(), notes.path())
        .expect("start the open file watcher");

    let outcome = watcher
        .registry()
        .lock()
        .expect("registry")
        .watch_parent_of("note-1", &file);
    assert_eq!(outcome, WatchOutcome::Watching(WatcherKind::Native));

    write_by_temp_and_rename(&file, b"rewritten by another program\n");

    let seen = collect_external(&rx);
    assert_eq!(
        seen.len(),
        1,
        "a temp-plus-rename must reach the tab exactly once, saw {seen:?}"
    );
    match &seen[0] {
        WritEvent::BufferExternal {
            buffer_id,
            path,
            change,
            new_path,
            disk_hash,
        } => {
            assert_eq!(buffer_id, "note-1");
            assert_eq!(
                resolved(Path::new(path)),
                resolved(&file),
                "the event names the file that changed"
            );
            assert_eq!(
                *change,
                writ_core::watcher::change_event::ExternalChange::Modified
            );
            assert_eq!(*new_path, None);
            assert_eq!(
                disk_hash.as_deref(),
                Some(
                    writ_core::hash::comparison_digest_hex(b"rewritten by another program\n")
                        .as_str()
                ),
                "the event carries what the file holds now"
            );
        }
        other => panic!("expected BufferExternal, got {other:?}"),
    }
}

#[test]
fn a_write_writ_stamped_never_comes_back_as_somebody_elses_edit() {
    // The full round trip for the rule W3 exists for: Writ saves the file it
    // opened, the folder watcher sees the write, and the stamp keyed by the
    // file's resolved path stops it becoming an offer to reload the text the
    // tab is already showing.
    let notes = TempDir::new().expect("notes dir");
    let elsewhere = TempDir::new().expect("some other folder");
    let file = elsewhere.path().join("mine.md");
    std::fs::write(&file, b"before\n").expect("seed file");

    let ignore = create_ignore_set();
    let (bus, rx) = bus_with_channel();
    let watcher = start_open_file_watcher(bus, ignore.clone(), notes.path())
        .expect("start the open file watcher");
    watcher
        .registry()
        .lock()
        .expect("registry")
        .watch_parent_of("note-1", &file);

    let saved = b"saved from writ\n";
    {
        let mut set = ignore.lock().expect("ignore set");
        set.record(
            writ_core::watcher::ignore::source_key(Path::new(&resolved(&file))),
            saved,
            Instant::now(),
        );
    }
    write_by_temp_and_rename(&file, saved);

    assert!(
        collect_external(&rx).is_empty(),
        "a stamped write must not arrive back as an external change"
    );
    assert!(
        DEFAULT_IGNORE_TTL > SETTLE,
        "this test only means anything while the stamp is still live"
    );
}

#[test]
fn a_folder_stops_being_watched_when_the_last_tab_in_it_closes() {
    let notes = TempDir::new().expect("notes dir");
    let elsewhere = TempDir::new().expect("some other folder");
    let file = elsewhere.path().join("closing.md");
    std::fs::write(&file, b"before\n").expect("seed file");

    let (bus, rx) = bus_with_channel();
    let watcher = start_open_file_watcher(bus, create_ignore_set(), notes.path())
        .expect("start the open file watcher");
    {
        let mut registry = watcher.registry().lock().expect("registry");
        registry.watch_parent_of("note-1", &file);
        registry.unwatch_parent_of("note-1");
        assert!(registry.watched_dirs().is_empty());
    }

    write_by_temp_and_rename(&file, b"changed after the tab closed\n");

    assert!(
        collect_external(&rx).is_empty(),
        "a closed tab's folder must not keep reporting"
    );
}

#[test]
fn a_folder_full_of_churn_never_names_a_file_that_is_not_open() {
    // A plugin or a build tool writing hundreds of files into a watched folder
    // must not turn into hundreds of messages. Two things bound it: only a
    // file that is an open note is ever named, and each note is named at most
    // once per delivered window.
    let notes = TempDir::new().expect("notes dir");
    let busy = TempDir::new().expect("a busy folder");
    let one = busy.path().join("one.md");
    let two = busy.path().join("two.md");
    std::fs::write(&one, b"before\n").expect("seed one");
    std::fs::write(&two, b"before\n").expect("seed two");

    let (bus, rx) = bus_with_channel();
    let watcher = start_open_file_watcher(bus, create_ignore_set(), notes.path())
        .expect("start the open file watcher");
    {
        let mut registry = watcher.registry().lock().expect("registry");
        registry.watch_parent_of("note-1", &one);
        registry.watch_parent_of("note-2", &two);
    }

    for round in 0..40 {
        std::fs::write(busy.path().join(format!("noise-{round}.log")), b"x").expect("noise");
        std::fs::write(&one, format!("round {round}\n")).expect("rewrite one");
        std::fs::write(&two, format!("round {round}\n")).expect("rewrite two");
    }

    let seen = collect_external(&rx);
    assert!(
        !seen.is_empty(),
        "the churn must still reach the two open tabs"
    );

    for event in &seen {
        let WritEvent::BufferExternal {
            buffer_id, path, ..
        } = event
        else {
            unreachable!("collect_external returns nothing else");
        };
        assert!(
            buffer_id == "note-1" || buffer_id == "note-2",
            "only an open note may be named, got {buffer_id} for {path}"
        );
    }

    // Two open notes, one event each per 500 ms window, over a run bounded by
    // the settle period. Anything near the 120 writes made would mean the
    // per-window cap is not doing its job.
    let windows = SETTLE.as_millis() / 500 + 1;
    let ceiling = (2 * windows) as usize;
    assert!(
        seen.len() <= ceiling,
        "120 writes must cost at most {ceiling} events, saw {}",
        seen.len()
    );
}

#[test]
fn a_note_inside_the_notes_folder_reaches_its_tab_when_another_program_rewrites_it() {
    // W1's headline behaviour on the folder that holds nearly every note.
    // The notes watcher is the only thing watching in there — the open-file
    // registry deliberately arms nothing over the notes tree — so the route
    // from a change to the tab holding that file runs through it. Before this,
    // the change reached the index and stopped: the tab went on showing text
    // its file no longer held, and its next save was refused by the write
    // guard.
    let notes = TempDir::new().expect("notes dir");
    let note = notes.path().join("today.md");
    std::fs::write(&note, b"original text\n").expect("seed note");

    let (bus, rx) = bus_with_channel();
    let ignore = create_ignore_set();
    let open_files = start_open_file_watcher(bus.clone(), ignore.clone(), notes.path())
        .expect("start the open file watcher");

    let outcome = open_files
        .registry()
        .lock()
        .expect("registry")
        .watch_parent_of("note-1", &note);
    assert_eq!(
        outcome,
        WatchOutcome::AlreadyCovered,
        "the notes watcher covers this folder; a second watch would double every event"
    );

    let _notes_watcher = start_notes_watcher(
        bus,
        canonical(notes.path()),
        ignore,
        open_files.open_notes(),
    )
    .expect("start the notes watcher");

    write_by_temp_and_rename(&note, b"rewritten by another program\n");

    let seen = collect_external(&rx);
    assert_eq!(seen.len(), 1, "the tab must be told once, saw {seen:?}");
    match &seen[0] {
        WritEvent::BufferExternal {
            buffer_id,
            path,
            change,
            new_path,
            disk_hash,
        } => {
            assert_eq!(buffer_id, "note-1");
            assert_eq!(resolved(Path::new(path)), resolved(&note));
            assert_eq!(
                *change,
                writ_core::watcher::change_event::ExternalChange::Modified
            );
            assert_eq!(*new_path, None);
            assert_eq!(
                disk_hash.as_deref(),
                Some(
                    writ_core::hash::comparison_digest_hex(b"rewritten by another program\n")
                        .as_str()
                ),
                "the tab compares its document against this digest, so it has to be the file's"
            );
        }
        other => panic!("expected BufferExternal, got {other:?}"),
    }
}

#[test]
fn a_save_writ_made_inside_the_notes_folder_never_comes_back_to_the_tab() {
    // The same round trip on the notes side. Writ stamps a write before it
    // lands; if the stamp missed, every save would return as somebody else's
    // edit and the user would be asked whether to discard their own keystrokes.
    assert!(
        DEFAULT_IGNORE_TTL > SETTLE,
        "the stamp has to outlive the wait, or this passes for the wrong reason"
    );

    let notes = TempDir::new().expect("notes dir");
    let note = notes.path().join("today.md");
    std::fs::write(&note, b"before\n").expect("seed note");

    let (bus, rx) = bus_with_channel();
    let ignore = create_ignore_set();
    let open_files = start_open_file_watcher(bus.clone(), ignore.clone(), notes.path())
        .expect("start the open file watcher");
    open_files
        .registry()
        .lock()
        .expect("registry")
        .watch_parent_of("note-1", &note);

    let _notes_watcher = start_notes_watcher(
        bus,
        canonical(notes.path()),
        ignore.clone(),
        open_files.open_notes(),
    )
    .expect("start the notes watcher");

    let saved = b"what writ itself wrote\n";
    {
        let mut set = ignore.lock().expect("ignore set");
        set.record(
            writ_core::watcher::ignore::source_key(Path::new(&resolved(&note))),
            saved,
            Instant::now(),
        );
    }
    write_by_temp_and_rename(&note, saved);

    let seen = collect_external(&rx);
    assert!(
        seen.is_empty(),
        "a stamped write must not be reported to the tab that made it, saw {seen:?}"
    );
}

#[test]
fn a_note_nobody_has_open_tells_no_tab() {
    // The registry is the whole filter. A notes folder holds every note the
    // user has; only the ones with a tab may produce an event.
    let notes = TempDir::new().expect("notes dir");
    let open = notes.path().join("open.md");
    let closed = notes.path().join("closed.md");
    std::fs::write(&open, b"x\n").expect("seed");
    std::fs::write(&closed, b"x\n").expect("seed");

    let (bus, rx) = bus_with_channel();
    let ignore = create_ignore_set();
    let open_files = start_open_file_watcher(bus.clone(), ignore.clone(), notes.path())
        .expect("start the open file watcher");
    open_files
        .registry()
        .lock()
        .expect("registry")
        .watch_parent_of("note-1", &open);

    let _notes_watcher = start_notes_watcher(
        bus,
        canonical(notes.path()),
        ignore,
        open_files.open_notes(),
    )
    .expect("start the notes watcher");

    write_by_temp_and_rename(&closed, b"changed by another program\n");

    let seen = collect_external(&rx);
    assert!(
        seen.is_empty(),
        "a note with no tab open on it has nobody to tell, saw {seen:?}"
    );
}
