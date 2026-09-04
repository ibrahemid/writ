use std::fs;
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

fn setup() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("failed to create temp dir");
    let db_path = dir.path().join("test.db");
    let conn = open_database(&db_path).expect("failed to open database");
    run_migrations(&conn).expect("migrations failed");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("failed to create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);
    (dir, store)
}

/// A note and the file it lives in, created empty so the row is openable.
fn make_note(dir: &Path, id: &str, title: &str) -> (BufferDocument, std::path::PathBuf) {
    let notes = dir.join("notes");
    fs::create_dir_all(&notes).expect("create notes dir");
    let file = notes.join(format!("{title}.md"));
    fs::write(&file, b"").expect("seed note file");
    (make_doc(id, title, &file), file)
}

fn make_doc(id: &str, title: &str, source_path: &Path) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(source_path.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    }
}

fn count_files_in_dir(dir: &Path) -> usize {
    fs::read_dir(dir)
        .expect("read_dir failed")
        .filter_map(|e| e.ok())
        .count()
}

#[test]
fn save_content_leaves_no_temp_files_behind() {
    let (dir, store) = setup();
    let (doc, _file) = make_note(dir.path(), "atomic-1", "no-leftovers");
    store.insert(&doc).expect("insert failed");

    let notes_dir = dir.path().join("notes");
    store
        .save_content("atomic-1", "first write")
        .expect("save_content failed");
    store
        .save_content("atomic-1", "second write")
        .expect("save_content failed");
    store
        .save_content("atomic-1", "third write")
        .expect("save_content failed");

    assert_eq!(
        count_files_in_dir(&notes_dir),
        1,
        "the notes folder holds the note and no .tmp leftovers"
    );
    assert!(
        count_files_in_dir(&dir.path().join("buffers")) == 0,
        "a save copies the text nowhere"
    );

    let content = store.read_content("atomic-1").expect("read_content failed");
    assert_eq!(content, "third write");
}

#[test]
fn save_content_does_not_truncate_destination_on_temp_failure() {
    let (dir, store) = setup();
    let (doc, dest) = make_note(dir.path(), "atomic-2", "preserve-original");
    store.insert(&doc).expect("insert failed");

    let original = "ORIGINAL CONTENT THAT MUST SURVIVE";
    store
        .save_content("atomic-2", original)
        .expect("initial save_content failed");

    let original_bytes = fs::read(&dest).expect("read dest failed");
    assert_eq!(original_bytes, original.as_bytes());

    // A regular file where the target's parent directory should be makes the
    // temp-file creation fail on every platform. (A read-only directory does
    // not block file creation on Windows, so the chmod trick is not portable.)
    let blocker = dir.path().join("not-a-dir");
    fs::write(&blocker, b"file, not a directory").expect("write blocker failed");

    let nested_target = blocker.join("nope.txt");
    let attempt = writ_storage::atomic::write_atomic(&nested_target, b"this should never land");
    assert!(
        attempt.is_err(),
        "writing under a non-directory parent must fail"
    );
    assert!(
        !nested_target.exists(),
        "no destination file should be created when the write fails"
    );

    let surviving = fs::read(&dest).expect("read dest again failed");
    assert_eq!(
        surviving, original_bytes,
        "destination contents must be unchanged after a failed write elsewhere"
    );
}

#[test]
fn write_atomic_replaces_existing_file_in_one_step() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("doc.txt");

    fs::write(&target, b"OLD VALUE").expect("seed failed");

    writ_storage::atomic::write_atomic(&target, b"NEW VALUE").expect("atomic write failed");

    let read_back = fs::read(&target).expect("read failed");
    assert_eq!(read_back, b"NEW VALUE");

    assert_eq!(
        count_files_in_dir(dir.path()),
        1,
        "only the target file should exist; temp files must be cleaned up"
    );
}

#[test]
fn write_atomic_creates_new_file_when_target_missing() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("fresh.txt");

    writ_storage::atomic::write_atomic(&target, b"FRESH").expect("atomic write failed");

    let read_back = fs::read(&target).expect("read failed");
    assert_eq!(read_back, b"FRESH");
    assert_eq!(count_files_in_dir(dir.path()), 1);
}

#[test]
fn write_atomic_preserves_original_when_parent_dir_missing() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("present").join("doc.txt");
    fs::create_dir_all(target.parent().unwrap()).expect("mkdir failed");
    fs::write(&target, b"KEEP ME").expect("seed failed");

    let missing = dir.path().join("does-not-exist").join("doc.txt");
    let attempt = writ_storage::atomic::write_atomic(&missing, b"nope");
    assert!(attempt.is_err(), "writing under missing parent must fail");

    let surviving = fs::read(&target).expect("read failed");
    assert_eq!(surviving, b"KEEP ME");
}

#[test]
fn save_to_source_is_atomic_for_external_file() {
    let (dir, store) = setup();

    let source_file = dir.path().join("external-source.txt");
    let original_external = "ORIGINAL EXTERNAL CONTENT".repeat(100);
    fs::write(&source_file, &original_external).expect("seed external failed");

    let now = Utc::now();
    let doc = BufferDocument {
        id: "atomic-src-1".to_string(),
        title: "external".to_string(),
        filename: "atomic-src-1.txt".to_string(),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(source_file.to_string_lossy().into_owned()),
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
    };
    store.insert(&doc).expect("insert failed");

    let updated = "UPDATED EXTERNAL CONTENT".repeat(100);
    store
        .save_to_source("atomic-src-1", &updated, None, None)
        .expect("save_to_source failed");

    let on_disk = fs::read_to_string(&source_file).expect("read source failed");
    assert_eq!(on_disk, updated);

    let source_dir = source_file.parent().unwrap();
    let leftover_tmp = fs::read_dir(source_dir)
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .any(|e| e.file_name().to_string_lossy().contains(".tmp"));
    assert!(
        !leftover_tmp,
        "no .tmp sibling files should remain next to the source file after a successful save"
    );
}

#[cfg(unix)]
#[test]
fn save_content_swaps_inode_proving_rename_into_place() {
    use std::os::unix::fs::MetadataExt;

    let (dir, store) = setup();
    let (doc, dest) = make_note(dir.path(), "inode-1", "rename-proof");
    store.insert(&doc).expect("insert failed");

    store
        .save_content("inode-1", "first version")
        .expect("first save failed");
    let first_inode = fs::metadata(&dest).expect("metadata 1").ino();

    store
        .save_content("inode-1", "second version with different length")
        .expect("second save failed");
    let second_inode = fs::metadata(&dest).expect("metadata 2").ino();

    assert_ne!(
        first_inode, second_inode,
        "rename-into-place must produce a fresh inode on each write; \
         identical inodes imply in-place truncation (the bug)"
    );

    let content = store.read_content("inode-1").expect("read_content failed");
    assert_eq!(content, "second version with different length");
}

#[test]
fn write_atomic_handles_large_payloads() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("big.bin");

    let mut payload = Vec::with_capacity(512 * 1024);
    for i in 0..(512u32 * 1024 / 4) {
        payload
            .write_all(&i.to_le_bytes())
            .expect("payload build failed");
    }

    writ_storage::atomic::write_atomic(&target, &payload).expect("atomic write failed");

    let read_back = fs::read(&target).expect("read failed");
    assert_eq!(read_back.len(), payload.len());
    assert_eq!(read_back, payload);
    assert_eq!(count_files_in_dir(dir.path()), 1);
}

#[cfg(unix)]
#[test]
fn write_atomic_keeps_the_destination_mode() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("script.sh");
    fs::write(&target, b"#!/bin/sh\n").expect("seed failed");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).expect("chmod failed");

    writ_storage::atomic::write_atomic(&target, b"#!/bin/sh\necho hi\n").expect("atomic write");

    let mode = fs::metadata(&target).expect("stat").permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o755,
        "a save must not narrow the file it replaces to the temp file's own mode"
    );
}

#[cfg(unix)]
#[test]
fn write_atomic_creates_new_files_privately() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("fresh.txt");

    writ_storage::atomic::write_atomic(&target, b"new").expect("atomic write");

    let mode = fs::metadata(&target).expect("stat").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "a file with no predecessor stays owner-only");
}

#[test]
fn write_atomic_round_trips_a_note_with_frontmatter_byte_for_byte() {
    let dir = TempDir::new().expect("failed to create temp dir");
    let target = dir.path().join("note.md");
    // Comment line, quoted value, unsorted keys and trailing whitespace: the
    // block must come back as authored, never reserialised (spec F3).
    let note = "---\n# a comment\ntitle: \"Quoted  value\"   \ntags:\t[x]  \nz_last: 1\na_first: 2\n---\nBody text\n";

    writ_storage::atomic::write_atomic(&target, note.as_bytes()).expect("atomic write failed");

    let read_back = fs::read(&target).expect("read note back");
    assert_eq!(read_back, note.as_bytes());
}

#[test]
fn the_temp_file_a_save_writes_carries_writs_prefix_and_is_ignored() {
    // The sibling is created and deleted inside every save, in the user's
    // notes folder. A name the ignore rules can match is what keeps that churn
    // out of the watcher, the index and the user's sync client.
    let dir = TempDir::new().expect("temp dir");
    let tmp = writ_storage::atomic::temp_sibling(dir.path()).expect("temp sibling");

    let name = tmp
        .path()
        .file_name()
        .expect("a temp file has a name")
        .to_string_lossy()
        .into_owned();

    assert!(
        name.starts_with(writ_core::workspace::TEMP_FILE_PREFIX),
        "temp files are named from the shared prefix, got {name}"
    );
    assert_ne!(
        name.as_str(),
        writ_core::workspace::TEMP_FILE_PREFIX,
        "the prefix is followed by the random part that keeps two saves apart"
    );
    assert!(
        writ_core::workspace::is_ignored_name(&name),
        "the name Writ writes has to be one the ignore rules answer for"
    );
    assert_eq!(tmp.path().parent(), Some(dir.path()));
}

#[test]
fn an_atomic_write_leaves_no_temp_file_behind() {
    let dir = TempDir::new().expect("temp dir");
    let target = dir.path().join("note.md");
    writ_storage::atomic::write_atomic(&target, b"body").expect("write");

    let names: Vec<String> = fs::read_dir(dir.path())
        .expect("read dir")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();

    assert_eq!(names, vec!["note.md".to_string()]);
}

// What a save has to carry across the rename that replaces the file, and the
// two destinations it must refuse rather than replace (ADR-028 §5).

/// Hangs an extended attribute on a path, the way `xattr -w` does.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn set_xattr(path: &Path, name: &str, value: &[u8]) {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).expect("path");
    let c_name = CString::new(name).expect("name");
    let result = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::setxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
                0,
            )
        }
        #[cfg(target_os = "linux")]
        {
            libc::setxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_ptr().cast(),
                value.len(),
                0,
            )
        }
    };
    assert_eq!(result, 0, "could not set {name}: {}", last_error());
}

/// Reads one extended attribute back, or `None` when the file has no such
/// attribute.
#[cfg(any(target_os = "macos", target_os = "linux"))]
fn read_xattr(path: &Path, name: &str) -> Option<Vec<u8>> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let c_path = CString::new(path.as_os_str().as_bytes()).expect("path");
    let c_name = CString::new(name).expect("name");
    let mut value = vec![0u8; 4096];
    let written = unsafe {
        #[cfg(target_os = "macos")]
        {
            libc::getxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
                0,
                0,
            )
        }
        #[cfg(target_os = "linux")]
        {
            libc::getxattr(
                c_path.as_ptr(),
                c_name.as_ptr(),
                value.as_mut_ptr().cast(),
                value.len(),
            )
        }
    };
    if written < 0 {
        return None;
    }
    value.truncate(written as usize);
    Some(value)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn last_error() -> std::io::Error {
    std::io::Error::last_os_error()
}

/// A test that changes a file's mode has to put it back, or `TempDir` cannot
/// remove what it made.
#[cfg(unix)]
fn restore_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
}

/// Root ignores the permission bits these tests rely on, so the refusals they
/// assert never happen there.
#[cfg(unix)]
fn running_as_root() -> bool {
    // SAFETY: `geteuid` reads the calling process's effective user id and
    // touches nothing.
    unsafe { libc::geteuid() == 0 }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_save_carries_the_files_tags_across() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("tagged.md");
    fs::write(&target, b"before").expect("seed");

    // A Finder tag, in the attribute Finder actually writes it to.
    set_xattr(&target, "com.apple.metadata:_kMDItemUserTags", b"Red\n");

    writ_storage::atomic::write_atomic(&target, b"after").expect("write");

    assert_eq!(fs::read(&target).expect("read"), b"after");
    assert_eq!(
        read_xattr(&target, "com.apple.metadata:_kMDItemUserTags").as_deref(),
        Some(&b"Red\n"[..]),
        "the tag on the file did not survive the save"
    );
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_save_drops_the_quarantine_record() {
    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("downloaded.md");
    fs::write(&target, b"before").expect("seed");

    set_xattr(&target, "com.apple.quarantine", b"0081;00000000;Writ;");
    set_xattr(&target, "com.apple.metadata:_kMDItemUserTags", b"Blue\n");

    writ_storage::atomic::write_atomic(&target, b"after").expect("write");

    assert_eq!(
        read_xattr(&target, "com.apple.quarantine"),
        None,
        "a file the user has been editing keeps being asked about"
    );
    assert_eq!(
        read_xattr(&target, "com.apple.metadata:_kMDItemUserTags").as_deref(),
        Some(&b"Blue\n"[..]),
        "skipping one attribute must not skip the rest"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn a_save_keeps_the_date_the_file_was_created() {
    use std::os::macos::fs::MetadataExt;

    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("old.md");
    fs::write(&target, b"before").expect("seed");
    let before = fs::metadata(&target).expect("stat").st_birthtime();

    // A second boundary is what makes the assertion mean something: without
    // the restore the file takes the temp file's date, which is now.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    writ_storage::atomic::write_atomic(&target, b"after").expect("write");

    let after = fs::metadata(&target).expect("stat").st_birthtime();
    assert_eq!(
        before, after,
        "every save would report the note as created moments ago"
    );
}

#[cfg(unix)]
#[test]
fn a_save_refuses_a_file_that_has_a_second_name() {
    let dir = TempDir::new().expect("tempdir");
    let first = dir.path().join("a.md");
    let second = dir.path().join("b.md");
    fs::write(&first, b"shared body").expect("seed");
    fs::hard_link(&first, &second).expect("link");

    let refusal =
        writ_storage::atomic::write_atomic(&first, b"new body").expect_err("the save must stop");
    assert!(
        matches!(
            refusal,
            writ_storage::atomic::AtomicWriteError::HardLinked { links: 2 }
        ),
        "{refusal:?}"
    );

    assert_eq!(fs::read(&first).expect("read"), b"shared body");
    assert_eq!(fs::read(&second).expect("read"), b"shared body");
    assert_eq!(
        count_files_in_dir(dir.path()),
        2,
        "a refusal must leave no temp sibling behind"
    );
}

#[cfg(unix)]
#[test]
fn a_save_addressed_to_the_real_file_leaves_the_link_pointing_at_it() {
    let dir = TempDir::new().expect("tempdir");
    let real = dir.path().join("real.md");
    let link = dir.path().join("link.md");
    fs::write(&real, b"before").expect("seed");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");

    writ_storage::atomic::write_atomic(&real, b"after").expect("write");

    assert_eq!(fs::read(&real).expect("read"), b"after");
    assert!(
        fs::symlink_metadata(&link)
            .expect("stat link")
            .file_type()
            .is_symlink(),
        "the link was replaced rather than followed"
    );
    assert_eq!(
        fs::read(&link).expect("read through the link"),
        b"after",
        "the link no longer points at the file it was made for"
    );
}

#[cfg(unix)]
#[test]
fn a_save_refuses_a_read_only_file() {
    use std::os::unix::fs::PermissionsExt;

    if running_as_root() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let target = dir.path().join("locked.md");
    fs::write(&target, b"untouched").expect("seed");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).expect("chmod");

    let refusal =
        writ_storage::atomic::write_atomic(&target, b"new body").expect_err("the save must stop");
    assert!(
        matches!(refusal, writ_storage::atomic::AtomicWriteError::ReadOnly),
        "{refusal:?}"
    );

    assert_eq!(fs::read(&target).expect("read"), b"untouched");
    assert_eq!(
        fs::metadata(&target).expect("stat").permissions().mode() & 0o777,
        0o444,
        "a refusal must not change the file's mode"
    );
    assert_eq!(
        count_files_in_dir(dir.path()),
        1,
        "a refusal must leave no temp sibling behind"
    );

    restore_mode(&target, 0o644);
}

#[cfg(unix)]
#[test]
fn a_save_refuses_a_read_only_folder() {
    use std::os::unix::fs::PermissionsExt;

    if running_as_root() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let folder = dir.path().join("locked-folder");
    fs::create_dir(&folder).expect("mkdir");
    let target = folder.join("note.md");
    fs::write(&target, b"untouched").expect("seed");
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o555)).expect("chmod");

    let refusal =
        writ_storage::atomic::write_atomic(&target, b"new body").expect_err("the save must stop");
    assert!(
        matches!(refusal, writ_storage::atomic::AtomicWriteError::ReadOnly),
        "{refusal:?}"
    );

    assert_eq!(fs::read(&target).expect("read"), b"untouched");
    assert_eq!(
        count_files_in_dir(&folder),
        1,
        "a refusal must leave no temp sibling behind"
    );

    restore_mode(&folder, 0o755);
}

#[cfg(unix)]
#[test]
fn a_refused_save_reaches_the_editor_named() {
    let (dir, store) = setup();
    let (doc, file) = make_note(dir.path(), "linked-1", "shared");
    store.insert(&doc).expect("insert");
    let second = file.with_file_name("shared copy.md");
    fs::hard_link(&file, &second).expect("link");

    let refusal = store
        .save_to_source("linked-1", "new body", None, None)
        .expect_err("the save must stop");

    match refusal {
        writ_storage::errors::StorageError::HardLinkedDestination { path, links } => {
            assert_eq!(path, file.to_string_lossy());
            assert_eq!(links, 2);
        }
        other => panic!("{other:?}"),
    }
    assert_eq!(fs::read(&file).expect("read"), b"");
}

#[cfg(unix)]
#[test]
fn a_refused_save_never_spends_an_entry_in_the_watcher_ignore_set() {
    use std::cell::RefCell;
    use std::os::unix::fs::PermissionsExt;

    if running_as_root() {
        return;
    }

    let (dir, store) = setup();
    let (doc, file) = make_note(dir.path(), "stamped-1", "locked");
    store.insert(&doc).expect("insert");

    // The stamp is what tells the watcher to ignore the next change to this
    // file. A refusal that spends one leaves the entry sitting there, and the
    // next change — somebody else's, the one the user needs to hear about —
    // is swallowed by it.
    let stamped: RefCell<Vec<std::path::PathBuf>> = RefCell::new(Vec::new());
    let record = |path: &Path, _bytes: &[u8]| stamped.borrow_mut().push(path.to_path_buf());

    fs::set_permissions(&file, fs::Permissions::from_mode(0o444)).expect("chmod");
    let refusal = store
        .save_to_source("stamped-1", "new body", None, Some(&record))
        .expect_err("the save must stop");
    assert!(
        matches!(
            refusal,
            writ_storage::errors::StorageError::DestinationReadOnly { .. }
        ),
        "{refusal:?}"
    );
    assert!(
        stamped.borrow().is_empty(),
        "a refused save stamped {:?}",
        stamped.borrow()
    );

    // The same closure on a save that goes through, so the assertion above
    // cannot be passing against a store that never stamps at all.
    restore_mode(&file, 0o644);
    store
        .save_to_source("stamped-1", "new body", None, Some(&record))
        .expect("save");
    assert_eq!(
        stamped.borrow().as_slice(),
        &[file.clone()],
        "a save that went through did not stamp the file it wrote"
    );
}
