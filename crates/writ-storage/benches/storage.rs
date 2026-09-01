use chrono::Utc;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{IndexedBy, IndexedNote, NotesIndex, NotesIndexStore};

const CORPUS_SIZE: usize = 500;

struct BenchDb {
    _dir: TempDir,
    index: NotesIndexStore,
}

fn make_doc(idx: usize) -> BufferDocument {
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
        source_path: None,
        cursor_pos: 0,
        scroll_pos: 0,
        tab_order: idx as u32,
        created_at: now,
        updated_at: now,
        closed_at: None,
        size_bytes: 0,
        read_only: false,
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

fn build_bench_db() -> BenchDb {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("bench.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");

    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    for idx in 0..CORPUS_SIZE {
        let doc = make_doc(idx);
        std::fs::write(
            notes.join(format!("{}.md", doc.id)),
            make_content(idx, 512 + (idx % 8) * 512),
        )
        .expect("write note");
    }
    drop(conn);

    let index = NotesIndexStore::open(&db_path).expect("index db");
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile");

    BenchDb { _dir: dir, index }
}

fn bench_notes_search(c: &mut Criterion) {
    let db = build_bench_db();

    let queries = ["rust", "editor buffer", "search index", "text file"];
    let mut group = c.benchmark_group("notes_search");
    for query in queries {
        group.bench_with_input(BenchmarkId::new("search", query), query, |b, q| {
            b.iter(|| {
                db.index.count(q).expect("search must not fail");
            });
        });
    }
    group.finish();
}

fn bench_buffer_round_trip(c: &mut Criterion) {
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

    let mut group = c.benchmark_group("buffer_round_trip");
    group.bench_function("save_4kb", |b| {
        b.iter(|| {
            store.save_content(&doc.id, &content).expect("save_content");
        });
    });
    group.bench_function("load_4kb", |b| {
        b.iter(|| {
            store.read_content(&doc.id).expect("read_content");
        });
    });
    group.finish();
}

fn bench_notes_index_upsert(c: &mut Criterion) {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("upd.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");

    let note = IndexedNote {
        path: dir.path().join("note.md").to_string_lossy().into_owned(),
        name: "note.md".to_string(),
        size: 4096,
        mtime: 0,
        hash: None,
        indexed_by: IndexedBy::Content,
    };
    let content = make_content(0, 4096);
    let index = NotesIndex::new(&conn);

    c.bench_function("notes_index_upsert_4kb", |b| {
        b.iter(|| {
            index.upsert(&note, &content).expect("upsert");
        });
    });
}

criterion_group!(
    benches,
    bench_notes_search,
    bench_buffer_round_trip,
    bench_notes_index_upsert
);
criterion_main!(benches);
