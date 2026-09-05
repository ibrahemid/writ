use std::time::Instant;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::file_ops::{generate_hex_dump, THRESHOLD_LARGE_BYTES, THRESHOLD_NORMAL_BYTES};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::NotesIndexStore;

const CORPUS_SIZE: usize = 500;
const MEDIAN_SAMPLES: usize = 9;
const FTS_BUDGET_MS: u128 = 200;
const FTS_PREFIX_BUDGET_MS: u128 = 50;
const ROUND_TRIP_BUDGET_MS: u128 = 50;
const OPEN_10MB_BUDGET_MS: u128 = 500;
const OPEN_50MB_BUDGET_MS: u128 = 4000;
const HEX_DUMP_10MB_BUDGET_MS: u128 = 1000;
/// Notes walked by the reconcile budget. ADR-028 section 7 states the keystroke
/// budget over a folder this size.
const RECONCILE_CORPUS: usize = 5_000;
const RECONCILE_BUDGET_MS: u128 = 30_000;
/// The keystroke budget from ADR-028 section 7: with a full reindex of 5,000
/// notes running, the first keystroke is served within these.
const KEYSTROKE_P95_BUDGET_MS: u128 = 50;
const KEYSTROKE_P99_BUDGET_MS: u128 = 150;
/// Searches timed against the running reconcile.
const KEYSTROKE_SAMPLES: usize = 200;

fn make_doc(notes: &std::path::Path, idx: usize) -> BufferDocument {
    let id = format!("buf-{:04}", idx);
    let words = [
        "rust", "editor", "buffer", "text", "search", "index", "file",
    ];
    let title = format!("{} note {}", words[idx % words.len()], idx);
    let now = Utc::now();
    BufferDocument {
        id: id.clone(),
        title: title.clone(),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        source_path: Some(
            notes
                .join(format!("{id}.md"))
                .to_string_lossy()
                .into_owned(),
        ),
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

/// One corpus note: frontmatter, a heading, a tag and links to two of its
/// neighbours, wrapped around [`make_content`].
///
/// The walk writes `links`, `properties`, `tags` and `headings` for every note
/// it reads (ADR-034), so a corpus of plain paragraphs would measure a walk
/// that has four fewer tables to fill than any real folder gives it.
fn make_note(idx: usize, count: usize, size_target: usize) -> String {
    let previous = (idx + count - 1) % count;
    let next = (idx + 1) % count;
    format!(
        "---\ntitle: Note {idx}\ntags: [corpus, generated]\n---\n\n\
         # Note {idx}\n\n\
         #corpus near [[note-{previous:05}]] and [[note-{next:05}]]\n\n\
         ## Body\n\n{}",
        make_content(idx, size_target)
    )
}

/// A folder of `count` notes plus a database with an empty index, the shape
/// the first launch after a migration finds.
fn build_notes_corpus(count: usize) -> (TempDir, std::path::PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("notes.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);

    let notes = dir.path().join("notes");
    // Fanned into subfolders: 5,000 entries in one directory is not the shape
    // a real notes folder has, and a flat walk would flatter the budget.
    for idx in 0..count {
        let folder = notes.join(format!("{:02}", idx % 50));
        std::fs::create_dir_all(&folder).expect("create folder");
        std::fs::write(
            folder.join(format!("note-{idx:05}.md")),
            make_note(idx, count, 512 + (idx % 8) * 512),
        )
        .expect("write note");
    }

    let index = NotesIndexStore::open(&db_path).expect("index db");
    (dir, notes, index)
}

/// A small notes corpus reconciled into the index, for the query budgets.
fn build_indexed_corpus() -> (TempDir, std::path::PathBuf, NotesIndexStore) {
    let (dir, notes, index) = build_notes_corpus(CORPUS_SIZE);
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile");
    (dir, notes, index)
}

/// The percentile of `samples` at `pct`, nearest-rank.
fn percentile_ms(samples: &[u128], pct: usize) -> u128 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = (pct * sorted.len()).div_ceil(100).max(1) - 1;
    sorted[rank]
}

fn build_corpus() -> (TempDir, BufferStore) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("perf.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    let buffers_dir = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers_dir).expect("create buffers dir");
    let notes_dir = dir.path().join("notes");
    std::fs::create_dir_all(&notes_dir).expect("create notes dir");
    let store = BufferStore::new(conn, buffers_dir);

    for idx in 0..CORPUS_SIZE {
        let doc = make_doc(&notes_dir, idx);
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
    let active_a = store_a.list_by_status(BufferStatus::Active).expect("list");
    assert_eq!(
        active_a.len(),
        CORPUS_SIZE,
        "corpus must contain exactly {} active buffers",
        CORPUS_SIZE,
    );

    let (_dir_b, store_b) = build_corpus();
    let active_b = store_b.list_by_status(BufferStatus::Active).expect("list");
    assert_eq!(
        active_a.len(),
        active_b.len(),
        "corpus size must be deterministic"
    );
    for (a, b) in active_a.iter().zip(active_b.iter()) {
        assert_eq!(a.id, b.id, "corpus ids must be deterministic");
    }
}

#[test]
fn fts_search_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let (_dir, _notes, index) = build_indexed_corpus();
    let queries = ["rust", "editor buffer", "search index", "text file"];

    for query in queries {
        let mut samples = Vec::with_capacity(MEDIAN_SAMPLES);
        for _ in 0..MEDIAN_SAMPLES {
            let start = Instant::now();
            index.count(query).expect("search must not fail");
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
fn fts_prefix_search_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let (_dir, _notes, index) = build_indexed_corpus();
    // The real search-as-you-type path issues quoted prefix terms built by
    // `writ_core::search::to_prefix_match`. Exercise that exact query shape so
    // the gate fails if the prefix index (migration 030) is dropped or the
    // prefix query stops resolving. Each prefix hits many corpus rows.
    let queries = [
        writ_core::search::to_prefix_match("rus").expect("query"),
        writ_core::search::to_prefix_match("edit").expect("query"),
        writ_core::search::to_prefix_match("buf").expect("query"),
        writ_core::search::to_prefix_match("sea").expect("query"),
    ];

    for query in &queries {
        // Sanity: the prefix query must actually match rows, or a budget pass
        // would be meaningless.
        assert!(
            index.count(query).expect("search must not fail") > 0,
            "prefix query '{}' matched nothing; corpus or index is wrong",
            query,
        );

        let mut samples = Vec::with_capacity(MEDIAN_SAMPLES);
        for _ in 0..MEDIAN_SAMPLES {
            let start = Instant::now();
            index.count(query).expect("search must not fail");
            samples.push(start.elapsed().as_millis());
        }
        let median = median_elapsed_ms(samples);
        assert!(
            median < FTS_PREFIX_BUDGET_MS,
            "fts prefix search '{}' median {}ms exceeds budget {}ms over {} buffers",
            query,
            median,
            FTS_PREFIX_BUDGET_MS,
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
    let notes_dir = dir.path().join("notes");
    std::fs::create_dir_all(&notes_dir).expect("create notes dir");
    let store = BufferStore::new(conn, buffers_dir);

    let doc = make_doc(&notes_dir, 0);
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

fn make_large_doc(notes: &std::path::Path, id: &str, size_bytes: u64) -> BufferDocument {
    let now = Utc::now();
    BufferDocument {
        id: id.to_string(),
        title: format!("{}.log", id),
        filename: format!("{}.txt", id),
        status: BufferStatus::Active,
        language: None,
        // A note without a file has nowhere to save to (ADR-028 section 1),
        // so the large-file rows carry one like every other row does.
        source_path: Some(
            notes
                .join(format!("{id}.log"))
                .to_string_lossy()
                .into_owned(),
        ),
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
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let store = BufferStore::new(conn, buffers_dir);

    let size = (THRESHOLD_NORMAL_BYTES + 1) as usize;
    let content = make_content(0, size);
    let doc = make_large_doc(&notes, "lg10", size as u64);
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
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let store = BufferStore::new(conn, buffers_dir);

    let size = THRESHOLD_LARGE_BYTES as usize;
    let content = make_content(0, size);
    let doc = make_large_doc(&notes, "lg50", size as u64);
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

#[test]
fn index_5000_notes_reconcile_stays_under_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let (_dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);

    let start = Instant::now();
    let outcome = index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile must not fail");
    let elapsed = start.elapsed().as_millis();

    assert_eq!(outcome.added, RECONCILE_CORPUS, "every note is indexed");
    assert!(
        elapsed < RECONCILE_BUDGET_MS,
        "reconcile of {} notes took {}ms, over the {}ms budget",
        RECONCILE_CORPUS,
        elapsed,
        RECONCILE_BUDGET_MS,
    );
    println!("reconcile {RECONCILE_CORPUS} notes: {elapsed}ms");
}

#[test]
fn search_hits_p95_under_50ms_and_p99_under_150ms_with_a_reconcile_running() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    // ADR-028 section 7: with a full reindex of 5,000 notes running, the first
    // keystroke is served within 50 ms at p95 and 150 ms at p99. The search
    // runs on its own connection, which is what makes that possible: the walk
    // commits in batches and WAL lets the reader through.
    let (_dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);
    let index = std::sync::Arc::new(index);

    let walker = {
        let index = index.clone();
        let notes = notes.clone();
        std::thread::spawn(move || {
            index
                .reconcile(&notes, &|| false, &|_| false)
                .expect("reconcile must not fail")
        })
    };

    let query = writ_core::search::to_prefix_match("rus").expect("query");
    let terms = writ_core::search::search_terms("rus");
    let mut samples = Vec::with_capacity(KEYSTROKE_SAMPLES);
    for _ in 0..KEYSTROKE_SAMPLES {
        let start = Instant::now();
        index
            .search_hits(&query, &terms, 100)
            .expect("search must not fail");
        samples.push(start.elapsed().as_millis());
    }

    walker.join().expect("reconcile thread");

    let p95 = percentile_ms(&samples, 95);
    let p99 = percentile_ms(&samples, 99);
    println!("search_hits under reconcile: p95 {p95}ms, p99 {p99}ms");
    assert!(
        p95 < KEYSTROKE_P95_BUDGET_MS,
        "p95 {}ms exceeds the {}ms budget with a reconcile running",
        p95,
        KEYSTROKE_P95_BUDGET_MS,
    );
    assert!(
        p99 < KEYSTROKE_P99_BUDGET_MS,
        "p99 {}ms exceeds the {}ms budget with a reconcile running",
        p99,
        KEYSTROKE_P99_BUDGET_MS,
    );
}
