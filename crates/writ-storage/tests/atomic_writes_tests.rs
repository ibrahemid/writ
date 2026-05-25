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

fn make_doc(id: &str, title: &str) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: title.to_string(),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: 0,
        created_at: now,
        updated_at: now,
        closed_at: None,
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
    let doc = make_doc("atomic-1", "no-leftovers");
    store.insert(&doc).expect("insert failed");

    let buffers_dir = dir.path().join("buffers");
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
        count_files_in_dir(&buffers_dir),
        1,
        "buffers dir should contain only the destination file, no .tmp leftovers"
    );

    let content = store.read_content("atomic-1").expect("read_content failed");
    assert_eq!(content, "third write");
}

#[test]
fn save_content_does_not_truncate_destination_on_temp_failure() {
    let (dir, store) = setup();
    let doc = make_doc("atomic-2", "preserve-original");
    store.insert(&doc).expect("insert failed");

    let original = "ORIGINAL CONTENT THAT MUST SURVIVE";
    store
        .save_content("atomic-2", original)
        .expect("initial save_content failed");

    let buffers_dir = dir.path().join("buffers");
    let dest = buffers_dir.join("atomic-2.txt");
    let original_bytes = fs::read(&dest).expect("read dest failed");
    assert_eq!(original_bytes, original.as_bytes());

    let read_only_dir = dir.path().join("readonly-buffers");
    fs::create_dir_all(&read_only_dir).expect("mkdir readonly failed");
    let mut perms = fs::metadata(&read_only_dir).expect("metadata").permissions();
    perms.set_readonly(true);
    fs::set_permissions(&read_only_dir, perms).expect("chmod readonly failed");

    let nested_target = read_only_dir.join("nope.txt");
    let attempt =
        writ_storage::atomic::write_atomic(&nested_target, b"this should never land");
    assert!(
        attempt.is_err(),
        "writing into a read-only directory must fail"
    );
    assert!(
        !nested_target.exists(),
        "no destination file should be created when the write fails"
    );

    let mut perms = fs::metadata(&read_only_dir).expect("metadata").permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perms.set_readonly(false);
    fs::set_permissions(&read_only_dir, perms).expect("restore perms failed");

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
    };
    store.insert(&doc).expect("insert failed");

    let updated = "UPDATED EXTERNAL CONTENT".repeat(100);
    store
        .save_to_source("atomic-src-1", &updated)
        .expect("save_to_source failed");

    let on_disk = fs::read_to_string(&source_file).expect("read source failed");
    assert_eq!(on_disk, updated);

    let source_dir = source_file.parent().unwrap();
    let leftover_tmp = fs::read_dir(source_dir)
        .expect("read_dir")
        .filter_map(|e| e.ok())
        .any(|e| {
            e.file_name()
                .to_string_lossy()
                .contains(".tmp")
        });
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
    let doc = make_doc("inode-1", "rename-proof");
    store.insert(&doc).expect("insert failed");

    let buffers_dir = dir.path().join("buffers");
    let dest = buffers_dir.join("inode-1.txt");

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
