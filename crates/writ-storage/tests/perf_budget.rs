use std::time::Instant;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::file_ops::{generate_hex_dump, THRESHOLD_LARGE_BYTES, THRESHOLD_NORMAL_BYTES};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;

const CORPUS_SIZE: usize = 500;
const MEDIAN_SAMPLES: usize = 9;
const FTS_BUDGET_MS: u128 = 200;
const ROUND_TRIP_BUDGET_MS: u128 = 50;
const OPEN_10MB_BUDGET_MS: u128 = 500;
const OPEN_50MB_BUDGET_MS: u128 = 4000;
const HEX_DUMP_10MB_BUDGET_MS: u128 = 1000;

fn make_doc(idx: usize) -> BufferDocument {
    let id = format!("buf-{:04}", idx);
    let words = ["rust", "editor", "buffer", "text", "search", "index", "file"];
    let title = format!("{} note {}", words[idx % words.len()], idx);
    let now = Utc::now();
    BufferDocument {
        id: id.clone(),
        title: title.clone(),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: idx as u32,
        created_at: now,
        updated_at: now,
        closed_at: None,
        read_only: false,
        size_bytes: 0,
    }
}

fn make_content(idx: usize, size_target: usize) -> String {
    let phrases: &[&str] = &[
        "the quick brown fox jumps over the lazy dog\n",
        "rust programming language systems performance\n",
        "text editor buffer management file operations\n",
        "full text search index query relevance rank\n",
        "sqlite database connection migration schema\n",
    ];
    let phrase = phrases[idx % phrases.len()];
    let mut buf = String::with_capacity(size_target + phrase.len());
    while buf.len() < size_target {
        buf.push_str(phrase);
    }
    buf
}

fn build_corpus() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("perf.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);

    for idx in 0..CORPUS_SIZE {
        let doc = make_doc(idx);
        store.insert(&doc).expect("insert");
        let content_size = 512 + (idx % 8) * 512;
        let content = make_content(idx, content_size);
        store.save_content(&doc.id, &content).expect("save_content");
    }

    (dir, store)
}

fn median_elapsed_ms(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

#[test]
fn corpus_fixture_deterministic_size() {
    let (_dir_a, store_a) = build_corpus();
    let active_a = store_a
        .list_by_status(BufferStatus::Active)
        .expect("list");
    assert_eq!(
        active_a.len(),
        CORPUS_SIZE,
        "corpus must contain exactly {} active buffers",
        CORPUS_SIZE,
    );

    let (_dir_b, store_b) = build_corpus();
    let active_b = store_b
        .list_by_status(BufferStatus::Active)
        .expect("list");
    assert_eq!(active_a.len(), active_b.len(), "corpus size must be deterministic");
    for (a, b) in active_a.iter().zip(active_b.iter()) {
        assert_eq!(a.id, b.id, "corpus ids must be deterministic");
    }
}

#[test]
fn fts_search_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let (_dir, store) = build_corpus();
    let queries = ["rust", "editor buffer", "search index", "text file"];

    for query in queries {
        let mut samples = Vec::with_capacity(MEDIAN_SAMPLES);
        for _ in 0..MEDIAN_SAMPLES {
            let start = Instant::now();
            store.search(query).expect("search must not fail");
            samples.push(start.elapsed().as_millis());
        }
        let median = median_elapsed_ms(samples);
        assert!(
            median < FTS_BUDGET_MS,
            "fts search '{}' median {}ms exceeds budget {}ms over {} buffers",
            query,
            median,
            FTS_BUDGET_MS,
            CORPUS_SIZE,
        );
    }
}

#[test]
fn buffer_round_trip_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("rt.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);

    let doc = make_doc(0);
    store.insert(&doc).expect("insert");
    let content = make_content(0, 4096);
    store.save_content(&doc.id, &content).expect("initial save");

    let mut save_samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for _ in 0..MEDIAN_SAMPLES {
        let start = Instant::now();
        store.save_content(&doc.id, &content).expect("save_content");
        save_samples.push(start.elapsed().as_millis());
    }
    let save_median = median_elapsed_ms(save_samples);
    assert!(
        save_median < ROUND_TRIP_BUDGET_MS,
        "save_content 4KB median {}ms exceeds budget {}ms",
        save_median,
        ROUND_TRIP_BUDGET_MS,
    );

    let mut load_samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for _ in 0..MEDIAN_SAMPLES {
        let start = Instant::now();
        store.read_content(&doc.id).expect("read_content");
        load_samples.push(start.elapsed().as_millis());
    }
    let load_median = median_elapsed_ms(load_samples);
    assert!(
        load_median < ROUND_TRIP_BUDGET_MS,
        "read_content 4KB median {}ms exceeds budget {}ms",
        load_median,
        ROUND_TRIP_BUDGET_MS,
    );
}

fn make_large_doc(id: &str, size_bytes: u64) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: format!("{}.log", id),
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
        read_only: false,
        size_bytes,
    }
}

#[test]
fn open_read_10mb_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("lg10.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);

    let size = (THRESHOLD_NORMAL_BYTES + 1) as usize;
    let content = make_content(0, size);
    let doc = make_large_doc("lg10", size as u64);
    store.insert(&doc).expect("insert");
    store.save_content(&doc.id, &content).expect("initial save");

    let mut read_samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for _ in 0..MEDIAN_SAMPLES {
        let start = Instant::now();
        store.read_content(&doc.id).expect("read_content");
        read_samples.push(start.elapsed().as_millis());
    }
    let read_median = median_elapsed_ms(read_samples);
    assert!(
        read_median < OPEN_10MB_BUDGET_MS,
        "read_content 10MB median {}ms exceeds budget {}ms",
        read_median,
        OPEN_10MB_BUDGET_MS,
    );
}

#[test]
fn open_read_50mb_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("lg50.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let store = BufferStore::new(conn, buffers_dir);

    let size = THRESHOLD_LARGE_BYTES as usize;
    let content = make_content(0, size);
    let doc = make_large_doc("lg50", size as u64);
    store.insert(&doc).expect("insert");
    store.save_content(&doc.id, &content).expect("initial save");

    let mut read_samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for _ in 0..MEDIAN_SAMPLES {
        let start = Instant::now();
        store.read_content(&doc.id).expect("read_content");
        read_samples.push(start.elapsed().as_millis());
    }
    let read_median = median_elapsed_ms(read_samples);
    assert!(
        read_median < OPEN_50MB_BUDGET_MS,
        "read_content 50MB median {}ms exceeds budget {}ms",
        read_median,
        OPEN_50MB_BUDGET_MS,
    );
}

#[test]
fn hex_dump_10mb_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let size = writ_core::file_ops::HEX_DUMP_MAX_BYTES;
    let data: Vec<u8> = (0..size).map(|i| (i % 256) as u8).collect();

    let mut samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for _ in 0..MEDIAN_SAMPLES {
        let start = Instant::now();
        let _ = generate_hex_dump(&data, size);
        samples.push(start.elapsed().as_millis());
    }
    let median = median_elapsed_ms(samples);
    assert!(
        median < HEX_DUMP_10MB_BUDGET_MS,
        "hex_dump 10MB median {}ms exceeds budget {}ms",
        median,
        HEX_DUMP_10MB_BUDGET_MS,
    );
}
