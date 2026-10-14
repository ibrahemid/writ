use std::path::{Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::notes_index::{self, IndexedBy, IndexedNote, NotesIndex, NotesIndexStore};

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
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        encoding: None,
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
    let notes = dir.path().join("notes");
    std::fs::create_dir_all(&notes).expect("create notes dir");
    let store = BufferStore::new(conn, buffers_dir);

    let mut doc = make_doc(0);
    doc.source_path = Some(
        notes
            .join(format!("{}.md", doc.id))
            .to_string_lossy()
            .into_owned(),
    );
    store.insert(&doc).expect("insert");
    // Two texts, so every timed save writes: the guard leaves a file that
    // already holds the incoming bytes alone.
    let texts = [make_content(0, 4096), make_content(1, 4096)];
    let mut state = store
        .save_to_source_without_index(&doc.id, &texts[0], None, None)
        .expect("initial save");
    let mut turn = 0usize;

    let mut group = c.benchmark_group("buffer_round_trip");
    group.bench_function("save_4kb", |b| {
        b.iter(|| {
            turn += 1;
            state = store
                .save_to_source_without_index(&doc.id, &texts[turn % 2], Some(state), None)
                .expect("guarded save");
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

/// The vault-scale folder: 10,000 notes, each writing twenty links, five of
/// them to one of the hundred notes most linked to. 200,000 links in all.
const VAULT_NOTES: usize = 10_000;
const VAULT_LINKS_PER_NOTE: usize = 20;
const VAULT_HUB_LINKS_PER_NOTE: usize = 5;
const VAULT_HUBS: usize = 100;

fn make_vault_note(idx: usize) -> String {
    let mut body = format!("# Note {idx}\n\n");
    for link in 0..VAULT_LINKS_PER_NOTE - VAULT_HUB_LINKS_PER_NOTE {
        let target = (idx * 31 + link * 997 + 1) % VAULT_NOTES;
        body.push_str(&format!("See [[vault-{target:05}]] for more.\n"));
    }
    for link in 0..VAULT_HUB_LINKS_PER_NOTE {
        let hub = (idx + link * 17) % VAULT_HUBS;
        body.push_str(&format!("Filed under [[vault-{hub:05}]].\n"));
    }
    body.push('\n');
    body.push_str(&make_content(idx, 256));
    body
}

fn vault_path(notes: &Path, idx: usize) -> PathBuf {
    notes
        .join(format!("{:02}", idx % 100))
        .join(format!("vault-{idx:05}.md"))
}

struct Vault {
    _dir: TempDir,
    notes: PathBuf,
    index: NotesIndexStore,
}

fn build_vault() -> Vault {
    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("vault.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let notes = dir.path().join("notes");
    for idx in 0..VAULT_NOTES {
        let path = vault_path(&notes, idx);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        std::fs::write(&path, make_vault_note(idx)).expect("write note");
    }
    let index = NotesIndexStore::open(&db_path).expect("index db");
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile");
    Vault {
        _dir: dir,
        notes,
        index,
    }
}

/// The index at 10,000 notes and 200,000 links: a save, a note arriving under
/// a name hundreds of links use, and the reads a keystroke or a panel makes
/// (EP-16). `docs/perf-budgets.md` holds the budget these are read against.
fn bench_vault_scale(c: &mut Criterion) {
    let vault = build_vault();
    let index = &vault.index;
    let mut group = c.benchmark_group("vault_10k_notes_200k_links");
    group
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3));

    let saved = vault_path(&vault.notes, 5000);
    let note = IndexedNote::from_file(&saved).expect("note");
    let texts = [make_vault_note(5000), make_vault_note(5001)];
    let mut turn = 0usize;
    group.bench_function("upsert_linked_note", |b| {
        b.iter(|| {
            turn += 1;
            index.upsert(&note, &texts[turn % 2]).expect("upsert");
        });
    });

    let twin = vault.notes.join("zz").join("vault-00007.md");
    std::fs::create_dir_all(twin.parent().expect("parent")).expect("create folder");
    std::fs::write(&twin, "# Twin\n").expect("write twin");
    group.bench_function("arrive_and_leave_under_a_much_linked_name", |b| {
        b.iter(|| {
            index.index_path(&twin).expect("index_path");
            index.forget_path(&twin).expect("forget_path");
        });
    });

    let hub = notes_index::index_key(&vault_path(&vault.notes, 7));
    group.bench_function("backlinks_of_a_much_linked_note", |b| {
        b.iter(|| index.backlinks(&hub).expect("backlinks"));
    });

    let from = notes_index::index_key(&vault_path(&vault.notes, 4321));
    group.bench_function("resolve_link", |b| {
        b.iter(|| index.resolve_link(&from, "vault-00007").expect("resolve"));
    });

    group.bench_function("quick_open_keystroke", |b| {
        b.iter(|| {
            index
                .search_names("vault-07", &vault.notes, 50)
                .expect("names")
        });
    });

    let query = writ_core::search::to_prefix_match("rus").expect("query");
    let terms = writ_core::search::search_terms("rus");
    group.bench_function("content_search_keystroke", |b| {
        b.iter(|| index.search_hits(&query, &terms, 100).expect("search"));
    });
    group.finish();
}

criterion_group!(
    benches,
    bench_notes_search,
    bench_buffer_round_trip,
    bench_notes_index_upsert,
    bench_vault_scale
);
criterion_main!(benches);
