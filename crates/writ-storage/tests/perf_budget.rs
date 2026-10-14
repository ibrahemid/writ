//! Storage budgets (`docs/perf-budgets.md`). Every timed test is skipped unless
//! `WRIT_PERF_GATE` is set; `scripts/perf-gate.sh` runs them in release, one at
//! a time.

use std::path::{Path, PathBuf};
use std::time::Instant;

use chrono::Utc;
use tempfile::TempDir;
use writ_core::buffer::document::{BufferDocument, BufferStatus};
use writ_core::buffer::manager::BufferManager;
use writ_core::file_ops::{generate_hex_dump, THRESHOLD_LARGE_BYTES, THRESHOLD_NORMAL_BYTES};
use writ_core::notes::links::Resolution;
use writ_core::notes::WriteOrigin;
use writ_storage::buffer_store::BufferStore;
use writ_storage::database::connection::open_database;
use writ_storage::database::migrations::run_migrations;
use writ_storage::note_ops;
use writ_storage::notes_index::{self, IndexedNote, NotesIndexStore};

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
/// The keystroke budget from ADR-028 section 7.
const KEYSTROKE_P95_BUDGET_MS: u128 = 50;
/// The ceiling every index-backed command issued during a full walk is held
/// to, measured as the slowest answer rather than a percentile: one command
/// that waits for the whole walk is the freeze, however many others got
/// through. The folder graph is held to the graph budget instead.
const MID_WALK_MAX_BUDGET_MS: u128 = 150;
/// Commands that must be issued while the walk is still running for the
/// mid-walk budget to have measured anything.
const MID_WALK_MIN_SAMPLES: usize = 20;
/// Saves timed by the per-save upsert budgets.
const UPSERT_SAMPLES: usize = 21;
/// The index's half of a save, at 5,000 notes: one note's rows, held to the
/// save budget whatever the size of the folder (RC-14).
const SAVE_UPSERT_BUDGET_MS: u128 = 50;
/// The vault-scale folder: notes, the links each one writes, and how many of
/// those go to the notes most linked to.
const VAULT_NOTES: usize = 10_000;
const VAULT_LINKS_PER_NOTE: usize = 20;
const VAULT_HUB_LINKS_PER_NOTE: usize = 5;
const VAULT_HUBS: usize = 100;
/// The vault's walk from an empty index.
const VAULT_RECONCILE_BUDGET_MS: u128 = 120_000;
/// A save, a new note, a note arriving under a name hundreds of links use, and
/// every read a keystroke or a panel issues, in a folder of 10,000 notes and
/// 200,000 links (EP-16).
const VAULT_WRITE_BUDGET_MS: u128 = 50;
const VAULT_READ_BUDGET_MS: u128 = 50;
/// Migration 045 over 200,000 stored links: the column, its index and every
/// row's key, once, at the first launch after the upgrade.
const NAME_KEY_MIGRATION_BUDGET_MS: u128 = 10_000;
/// Notes the renamed note's links are rewritten in, besides its two
/// neighbours in the corpus.
const RENAME_LINKING_NOTES: usize = 20;
/// What renaming a note with its links costs in a folder of 5,000 notes,
/// besides the rewrites, each of which is a guarded save held to the save
/// budget (RC-47).
const RENAME_BUDGET_MS: u128 = 300;

fn make_doc(notes: &Path, idx: usize) -> BufferDocument {
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

/// Writes `count` corpus notes under `notes`, fanned into fifty folders: 5,000
/// entries in one directory is not the shape a real notes folder has, and a
/// flat walk would flatter the budget.
fn write_notes_corpus(notes: &Path, count: usize) {
    for idx in 0..count {
        let folder = notes.join(format!("{:02}", idx % 50));
        std::fs::create_dir_all(&folder).expect("create folder");
        std::fs::write(
            folder.join(format!("note-{idx:05}.md")),
            make_note(idx, count, 512 + (idx % 8) * 512),
        )
        .expect("write note");
    }
}

/// A database with an empty index, the shape the first launch after a
/// migration finds.
fn empty_index(dir: &TempDir) -> (PathBuf, NotesIndexStore) {
    let db_path = dir.path().join("notes.db");
    let conn = open_database(&db_path).expect("open_database");
    run_migrations(&conn).expect("migrations");
    drop(conn);
    let index = NotesIndexStore::open(&db_path).expect("index db");
    (db_path, index)
}

/// A folder of `count` notes plus a database with an empty index.
fn build_notes_corpus(count: usize) -> (TempDir, PathBuf, NotesIndexStore) {
    let dir = TempDir::new().expect("tempdir");
    let notes = dir.path().join("notes");
    write_notes_corpus(&notes, count);
    let (_, index) = empty_index(&dir);
    (dir, notes, index)
}

/// A small notes corpus reconciled into the index, for the query budgets.
fn build_indexed_corpus() -> (TempDir, PathBuf, NotesIndexStore) {
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

fn median_elapsed_ms(mut samples: Vec<u128>) -> u128 {
    samples.sort_unstable();
    samples[samples.len() / 2]
}

/// Every file under `root`, by its path below it, with its bytes.
fn tree_contents(root: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files = Vec::new();
    let mut folders = vec![root.to_path_buf()];
    while let Some(folder) = folders.pop() {
        for entry in std::fs::read_dir(&folder).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                folders.push(path);
            } else {
                let relative = path
                    .strip_prefix(root)
                    .expect("under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                files.push((relative, std::fs::read(&path).expect("read")));
            }
        }
    }
    files.sort();
    files
}

#[test]
fn notes_corpus_is_deterministic() {
    let first = TempDir::new().expect("tempdir");
    let second = TempDir::new().expect("tempdir");
    write_notes_corpus(first.path(), CORPUS_SIZE);
    write_notes_corpus(second.path(), CORPUS_SIZE);

    let a = tree_contents(first.path());
    let b = tree_contents(second.path());
    assert_eq!(a.len(), CORPUS_SIZE, "one file per corpus note");
    assert!(
        a == b,
        "two builds of the corpus must be byte for byte the same"
    );
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
            "fts search '{}' median {}ms exceeds budget {}ms over {} notes",
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
            "fts prefix search '{}' median {}ms exceeds budget {}ms over {} notes",
            query,
            median,
            FTS_PREFIX_BUDGET_MS,
            CORPUS_SIZE,
        );
    }
}

/// The save the app makes: the guarded write of the note's file, judged
/// against what the last save left (ADR-028 §5), and the read back that opens
/// it. The index's half of the save is
/// [`per_save_upsert_at_5000_notes_stays_under_budget`].
#[test]
fn guarded_save_round_trip_budget() {
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
    // Two texts, so every timed save writes: the guard leaves a file that
    // already holds the incoming bytes alone.
    let texts = [make_content(0, 4096), make_content(1, 4096)];
    let mut state = store
        .save_to_source_without_index(&doc.id, &texts[0], None, None)
        .expect("initial save");

    let mut save_samples = Vec::with_capacity(MEDIAN_SAMPLES);
    for sample in 0..MEDIAN_SAMPLES {
        let text = &texts[(sample + 1) % 2];
        let start = Instant::now();
        state = store
            .save_to_source_without_index(&doc.id, text, Some(state), None)
            .expect("guarded save");
        save_samples.push(start.elapsed().as_millis());
    }
    let save_median = median_elapsed_ms(save_samples);
    assert!(
        save_median < ROUND_TRIP_BUDGET_MS,
        "guarded save 4KB median {}ms exceeds budget {}ms",
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

fn make_large_doc(notes: &Path, id: &str, size_bytes: u64) -> BufferDocument {
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
        line_ending: writ_core::notes::line_ending::LineEnding::Lf,
        encoding: None,
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
fn graph_over_5000_notes_stays_under_the_query_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    // The graph is one read of `files` and one grouped read of `links`, so it
    // is held to the query budget a keystroke is held to rather than to the
    // walk's: a folder graph the user opens must not cost what indexing costs.
    let (_dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile must not fail");

    let rows = index.graph(&notes).expect("graph must not fail");
    assert_eq!(rows.nodes.len(), RECONCILE_CORPUS, "every note is a node");
    assert!(
        !rows.edges.is_empty(),
        "the corpus links its notes together"
    );
    let (median, max) = time_runs(MEDIAN_SAMPLES, |_| {
        index.graph(&notes).expect("graph must not fail");
    });
    println!(
        "graph over {RECONCILE_CORPUS} notes: median {median}ms, max {max}ms of {MEDIAN_SAMPLES}, {} edges",
        rows.edges.len()
    );
    assert!(
        median < KEYSTROKE_P95_BUDGET_MS,
        "graph over {} notes: median {}ms, over the {}ms budget",
        RECONCILE_CORPUS,
        median,
        KEYSTROKE_P95_BUDGET_MS,
    );
}

/// One command the mid-walk budget times, named for the surface that issues it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MidWalkCommand {
    /// A content search keystroke in the sidebar.
    Search,
    /// Quick open (Cmd+Shift+O) and `[[` completion, which read one name index.
    Names,
    /// The backlink list beside the open note.
    Backlinks,
    /// The editor and preview resolving a `[[link]]`.
    Resolve,
    /// The graph of the whole folder, held to the graph budget rather than to
    /// the ceiling: it reads every note, so its time is the folder's.
    Graph,
    /// Cmd+N's storage writes: the file, the note's row through the app's
    /// own connection, and the note in the index.
    NewNote,
}

const MID_WALK_COMMANDS: [MidWalkCommand; 6] = [
    MidWalkCommand::Search,
    MidWalkCommand::Names,
    MidWalkCommand::Backlinks,
    MidWalkCommand::Resolve,
    MidWalkCommand::Graph,
    MidWalkCommand::NewNote,
];

/// Graph calls that must be issued during the walk for their median to mean
/// something.
const MID_WALK_MIN_GRAPH_SAMPLES: usize = 3;

/// One Cmd+N issued during the walk: its storage steps' times, and how many
/// entries the walk reached while it ran.
struct NewNoteSample {
    create: u128,
    record: u128,
    index: u128,
    walk_entries: usize,
}

#[test]
fn index_backed_commands_answer_within_budget_while_a_walk_holds_the_index() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;

    // ADR-028 section 7: with a full reindex of 5,000 notes running, the
    // commands a person reaches for keep answering. Every command is issued
    // only once the walk has started, and the slowest one is the measure: a
    // percentile discards exactly the command that waited for the walk.
    let (dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);
    let index = Arc::new(index);
    let entries = Arc::new(AtomicUsize::new(0));
    let finished = Arc::new(AtomicBool::new(false));

    let walker = {
        let index = index.clone();
        let notes = notes.clone();
        let entries = entries.clone();
        let finished = finished.clone();
        std::thread::spawn(move || {
            // Polled once per entry, so the first call is the walk under way
            // and the count is the entries it has reached.
            let cancelled = || {
                entries.fetch_add(1, Ordering::SeqCst);
                false
            };
            let outcome = index
                .reconcile(&notes, &cancelled, &|_| false)
                .expect("reconcile must not fail");
            finished.store(true, Ordering::SeqCst);
            outcome
        })
    };

    // The app's own connection, wired to the index the way the app wires it,
    // so Cmd+N's row competes with the walk for the database as it does there.
    let buffers = dir.path().join("buffers");
    std::fs::create_dir_all(&buffers).expect("create buffers dir");
    let mut store = BufferStore::new(
        open_database(&dir.path().join("notes.db")).expect("open app connection"),
        buffers,
    );
    store.set_notes_root(notes.clone());
    store.set_notes_index(index.clone());
    let mut manager = BufferManager::new();

    let query = writ_core::search::to_prefix_match("rus").expect("query");
    let terms = writ_core::search::search_terms("rus");
    let linked = notes_index::index_key(&notes.join("02").join("note-00002.md"));
    let linking = notes_index::index_key(&notes.join("01").join("note-00001.md"));
    let fresh = notes.join("zz-new");
    std::fs::create_dir_all(&fresh).expect("create folder for new notes");

    while entries.load(Ordering::SeqCst) == 0 {
        std::thread::yield_now();
    }

    let mut mid_walk: Vec<(MidWalkCommand, u128)> = Vec::new();
    let mut new_notes: Vec<NewNoteSample> = Vec::new();
    let mut issued = 0usize;
    while !finished.load(Ordering::SeqCst) {
        let command = MID_WALK_COMMANDS[issued % MID_WALK_COMMANDS.len()];
        let entries_before = entries.load(Ordering::SeqCst);
        let start = Instant::now();
        match command {
            MidWalkCommand::Search => {
                index
                    .search_hits(&query, &terms, 100)
                    .expect("search must not fail");
            }
            MidWalkCommand::Names => {
                index
                    .search_names("note-0", &notes, 50)
                    .expect("name search must not fail");
            }
            MidWalkCommand::Backlinks => {
                index.backlinks(&linked).expect("backlinks must not fail");
            }
            MidWalkCommand::Resolve => {
                index
                    .resolve_link(&linking, "note-00002")
                    .expect("resolve must not fail");
            }
            MidWalkCommand::Graph => {
                index.graph(&notes).expect("graph must not fail");
            }
            MidWalkCommand::NewNote => {
                let path = note_ops::create_note(
                    &fresh,
                    &format!("new-{issued:05}"),
                    "md",
                    WriteOrigin::Editor,
                    None,
                )
                .expect("create the note's file");
                let created = start.elapsed().as_millis();
                let doc = manager
                    .open_external(path.to_string_lossy().into_owned())
                    .expect("open the new note");
                let recording = Instant::now();
                store.open_from_path(&doc, "").expect("record the new note");
                let record = recording.elapsed().as_millis();
                let indexing = Instant::now();
                index.index_path(&path).expect("index_path must not fail");
                new_notes.push(NewNoteSample {
                    create: created,
                    record,
                    index: indexing.elapsed().as_millis(),
                    walk_entries: entries.load(Ordering::SeqCst) - entries_before,
                });
            }
        }
        let elapsed = start.elapsed().as_millis();
        issued += 1;
        mid_walk.push((command, elapsed));
    }

    let outcome = walker.join().expect("reconcile thread");
    assert!(
        outcome.added >= RECONCILE_CORPUS,
        "the walk indexed {} notes of {}",
        outcome.added,
        RECONCILE_CORPUS
    );

    for command in MID_WALK_COMMANDS {
        let times: Vec<u128> = mid_walk
            .iter()
            .filter(|(issued, _)| *issued == command)
            .map(|(_, ms)| *ms)
            .collect();
        println!(
            "mid-walk {command:?}: {} issued, max {}ms",
            times.len(),
            times.iter().max().copied().unwrap_or(0)
        );
    }
    if let Some(slowest) = new_notes
        .iter()
        .max_by_key(|sample| sample.create + sample.record + sample.index)
    {
        println!(
            "mid-walk slowest NewNote: file {}ms, row {}ms, index {}ms, {} walk entries meanwhile; \
             most walk entries during one NewNote: {}",
            slowest.create,
            slowest.record,
            slowest.index,
            slowest.walk_entries,
            new_notes.iter().map(|sample| sample.walk_entries).max().unwrap_or(0)
        );
    }

    let mut graph: Vec<u128> = mid_walk
        .iter()
        .filter(|(issued, _)| *issued == MidWalkCommand::Graph)
        .map(|(_, ms)| *ms)
        .collect();
    graph.sort_unstable();
    let mut samples: Vec<u128> = mid_walk
        .iter()
        .filter(|(issued, _)| *issued != MidWalkCommand::Graph)
        .map(|(_, ms)| *ms)
        .collect();
    samples.sort_unstable();
    let max = samples.last().copied().unwrap_or(0);
    println!(
        "mid-walk: {} commands besides the graph issued during the walk, p95 {}ms, max {}ms",
        samples.len(),
        if samples.is_empty() {
            0
        } else {
            percentile_ms(&samples, 95)
        },
        max
    );

    assert!(
        max < MID_WALK_MAX_BUDGET_MS,
        "a command issued during the walk took {}ms, over the {}ms budget",
        max,
        MID_WALK_MAX_BUDGET_MS,
    );
    assert!(
        samples.len() >= MID_WALK_MIN_SAMPLES,
        "only {} commands were issued during the walk; the budget measured nothing",
        samples.len(),
    );
    assert!(
        percentile_ms(&samples, 95) < KEYSTROKE_P95_BUDGET_MS,
        "p95 {}ms exceeds the {}ms budget during the walk",
        percentile_ms(&samples, 95),
        KEYSTROKE_P95_BUDGET_MS,
    );
    assert!(
        graph.len() >= MID_WALK_MIN_GRAPH_SAMPLES,
        "only {} graphs were drawn during the walk",
        graph.len(),
    );
    let graph_median = median_elapsed_ms(graph);
    println!("mid-walk graph median {graph_median}ms");
    assert!(
        graph_median < KEYSTROKE_P95_BUDGET_MS,
        "the graph's median during the walk was {}ms, over the {}ms graph budget",
        graph_median,
        KEYSTROKE_P95_BUDGET_MS,
    );
}

/// Times `samples` runs of `op`, returning the median and the slowest.
fn time_runs(samples: usize, mut op: impl FnMut(usize)) -> (u128, u128) {
    let mut times = Vec::with_capacity(samples);
    for sample in 0..samples {
        let start = Instant::now();
        op(sample);
        times.push(start.elapsed().as_millis());
    }
    let max = times.iter().max().copied().unwrap_or(0);
    (median_elapsed_ms(times), max)
}

#[test]
fn per_save_upsert_at_5000_notes_stays_under_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    // The deferred reindex behind every save writes one note's rows. Before
    // the names were kept in memory it read every path in `files` and every
    // row in `links` to do it, so its cost grew with the folder (RC-14).
    let (_dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile must not fail");

    let path = notes.join("07").join("note-02507.md");
    let note = IndexedNote::from_file(&path).expect("note");
    let texts = [
        make_note(2507, RECONCILE_CORPUS, 2048),
        make_note(2508, RECONCILE_CORPUS, 2048),
    ];
    let (median, max) = time_runs(UPSERT_SAMPLES, |sample| {
        index
            .upsert(&note, &texts[sample % 2])
            .expect("upsert must not fail");
    });
    println!("upsert at {RECONCILE_CORPUS} notes: median {median}ms, max {max}ms");
    assert!(
        max < SAVE_UPSERT_BUDGET_MS,
        "a save's upsert at {} notes took {}ms, over the {}ms budget",
        RECONCILE_CORPUS,
        max,
        SAVE_UPSERT_BUDGET_MS,
    );
}

#[test]
fn a_rename_with_its_links_at_5000_notes_stays_under_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    use writ_core::notes::links;
    use writ_storage::note_ops::{LinkRewrite, RewriteTarget};

    // What renaming a note with its links asks of storage, in the order the
    // rename command asks it: the notes linking to it and every name a link
    // could reach, from the index; the walk for files of the same name the
    // index does not hold; the move; a guarded rewrite of every linking note;
    // and the index brought in line with all of it. Everything but the
    // rewrites is timed: that is the part that grows with the folder, and
    // each rewrite is a save, which the save budget already holds.
    let (_dir, notes, index) = build_notes_corpus(RECONCILE_CORPUS);
    let linking_folder = notes.join("linking");
    std::fs::create_dir_all(&linking_folder).expect("create folder");
    for idx in 0..RENAME_LINKING_NOTES {
        std::fs::write(
            linking_folder.join(format!("linking-{idx:02}.md")),
            format!("# Linking {idx}\n\nsee [[note-02500]] for the plan\n"),
        )
        .expect("write linking note");
    }
    index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile must not fail");
    let target = notes.join("00").join("note-02500.md");

    let start = Instant::now();
    let key = notes_index::index_key(&target);
    let linking: std::collections::BTreeSet<String> = index
        .links_to(&key)
        .expect("links_to must not fail")
        .into_iter()
        .map(|row| row.from_path)
        .collect();
    let candidates = index.note_paths().expect("note_paths must not fail");
    let unindexed: Vec<String> = note_ops::files_named(&notes, &links::candidate_name_keys(&key))
        .iter()
        .map(|file| notes_index::index_key(file))
        .filter(|file| *file != key && !candidates.contains(file))
        .collect();
    let renamed = note_ops::rename_note(&target, "Renamed", None, WriteOrigin::Editor, None)
        .expect("rename must not fail");
    let before_rewrites = start.elapsed().as_millis();

    let rewrites_start = Instant::now();
    let rename = RewriteTarget {
        target: &key,
        new_name: "Renamed",
        candidates: &candidates,
        unindexed: &unindexed,
    };
    for file in &linking {
        let outcome =
            note_ops::rewrite_links_in_file(Path::new(file), &rename, None, None, None, None)
                .expect("rewrite must not fail");
        assert_eq!(outcome, LinkRewrite::Written, "{file}");
    }
    let rewrites = rewrites_start.elapsed().as_millis();

    let index_start = Instant::now();
    index
        .forget_path(&target)
        .expect("forget_path must not fail");
    index
        .index_path(&renamed)
        .expect("index_path must not fail");
    for file in &linking {
        index
            .index_path(Path::new(file))
            .expect("index_path must not fail");
    }
    let elapsed = before_rewrites + index_start.elapsed().as_millis();

    assert_eq!(linking.len(), RENAME_LINKING_NOTES + 2);
    assert_eq!(
        index
            .links_to(&notes_index::index_key(&renamed))
            .expect("links_to")
            .len(),
        RENAME_LINKING_NOTES + 2,
        "every link follows the rename"
    );
    println!(
        "rename with {} linking notes at {RECONCILE_CORPUS} notes: {elapsed}ms, \
         and {rewrites}ms in the rewrites",
        linking.len()
    );
    assert!(
        elapsed < RENAME_BUDGET_MS,
        "a rename with its links at {} notes took {}ms besides its rewrites, over the {}ms budget",
        RECONCILE_CORPUS,
        elapsed,
        RENAME_BUDGET_MS,
    );
}

/// Note `idx` of the vault: a heading, fifteen links spread over the folder,
/// five to the most-linked notes, and a paragraph.
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

#[test]
fn vault_of_10000_notes_and_200000_links_stays_under_budget() {
    if std::env::var("WRIT_PERF_GATE").is_err() {
        return;
    }

    let dir = TempDir::new().expect("tempdir");
    let notes = dir.path().join("notes");
    for idx in 0..VAULT_NOTES {
        let path = vault_path(&notes, idx);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("create folder");
        std::fs::write(&path, make_vault_note(idx)).expect("write note");
    }
    let (db_path, index) = empty_index(&dir);

    let start = Instant::now();
    let outcome = index
        .reconcile(&notes, &|| false, &|_| false)
        .expect("reconcile must not fail");
    let walk_ms = start.elapsed().as_millis();
    assert_eq!(outcome.added, VAULT_NOTES);
    let conn = open_database(&db_path).expect("open_database");
    let (links, unresolved): (i64, i64) = conn
        .query_row(
            "SELECT count(*), count(*) - count(to_path) FROM links",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("count links");
    assert_eq!(links as usize, VAULT_NOTES * VAULT_LINKS_PER_NOTE);
    assert_eq!(unresolved, 0, "every link names a note the walk indexed");
    println!("vault walk: {VAULT_NOTES} notes, {links} links, {walk_ms}ms");
    assert!(
        walk_ms < VAULT_RECONCILE_BUDGET_MS,
        "the vault walk took {walk_ms}ms, over the {VAULT_RECONCILE_BUDGET_MS}ms budget"
    );

    // Migration 045 over the same rows: the shape a database written before it
    // has, migrated forward. The runner applies only versions above the
    // highest one recorded, so 046 to 048 go too.
    conn.execute_batch(
        "DROP INDEX idx_links_name_key;
         ALTER TABLE links DROP COLUMN name_key;
         DROP TABLE unsaved_text;
         ALTER TABLE buffers DROP COLUMN preview_scripts_hash;
         ALTER TABLE buffers DROP COLUMN preview_scripts;
         DELETE FROM schema_version WHERE version >= 45;",
    )
    .expect("roll back to before 045");
    let start = Instant::now();
    run_migrations(&conn).expect("migrate to 045");
    let migration_ms = start.elapsed().as_millis();
    let keyed: i64 = conn
        .query_row(
            "SELECT count(*) FROM links WHERE name_key IS NOT NULL",
            [],
            |row| row.get(0),
        )
        .expect("count keys");
    assert_eq!(keyed, links);
    println!("name-key migration over {links} links: {migration_ms}ms");
    assert!(
        migration_ms < NAME_KEY_MIGRATION_BUDGET_MS,
        "migration 045 took {migration_ms}ms, over the {NAME_KEY_MIGRATION_BUDGET_MS}ms budget"
    );
    drop(conn);

    let hub = notes_index::index_key(&vault_path(&notes, 7));
    let from = notes_index::index_key(&vault_path(&notes, 4321));
    let mut results: Vec<(&str, u128, u128)> = Vec::new();

    let saved = vault_path(&notes, 5000);
    let note = IndexedNote::from_file(&saved).expect("note");
    let texts = [make_vault_note(5000), make_vault_note(5001)];
    // The migration above committed through another connection without
    // adding or removing a note, so the first save is timed like the rest.
    let (median, max) = time_runs(UPSERT_SAMPLES, |sample| {
        index
            .upsert(&note, &texts[sample % 2])
            .expect("upsert must not fail");
    });
    results.push(("save of a linked note", median, max));

    let fresh = notes.join("zz");
    std::fs::create_dir_all(&fresh).expect("create folder");
    let (median, max) = time_runs(UPSERT_SAMPLES, |sample| {
        let path = fresh.join(format!("fresh-{sample:03}.md"));
        std::fs::write(&path, "# Fresh\n\n[[vault-00007]]\n").expect("write");
        assert!(index.index_path(&path).expect("index_path"));
    });
    results.push(("Cmd+N, a new note", median, max));

    // A second note named like the most linked one: every link naming it
    // turns ambiguous, then back when the copy goes.
    let twin = fresh.join("vault-00007.md");
    std::fs::write(&twin, "# Twin\n").expect("write twin");
    let (median, max) = time_runs(UPSERT_SAMPLES, |sample| {
        if sample % 2 == 0 {
            assert!(index.index_path(&twin).expect("index_path"));
        } else {
            index.forget_path(&twin).expect("forget_path");
        }
    });
    results.push(("a second note of a much-linked name", median, max));
    index.forget_path(&twin).expect("forget_path");

    let (median, max) = time_runs(UPSERT_SAMPLES, |_| {
        let rows = index.backlinks(&hub).expect("backlinks");
        assert!(
            rows.len() > 100,
            "the hub is linked to from hundreds of notes"
        );
    });
    results.push(("backlinks of a much-linked note", median, max));

    let (median, max) = time_runs(UPSERT_SAMPLES, |_| {
        assert!(matches!(
            index.resolve_link(&from, "vault-00007").expect("resolve"),
            Resolution::Resolved(_)
        ));
    });
    results.push(("a link resolved", median, max));

    let (median, max) = time_runs(UPSERT_SAMPLES, |_| {
        index.search_names("vault-07", &notes, 50).expect("names");
    });
    results.push(("quick open keystroke", median, max));

    let query = writ_core::search::to_prefix_match("rus").expect("query");
    let terms = writ_core::search::search_terms("rus");
    let (median, max) = time_runs(UPSERT_SAMPLES, |_| {
        index.search_hits(&query, &terms, 100).expect("search");
    });
    results.push(("content search keystroke", median, max));

    for (what, median, max) in &results {
        println!("vault: {what}: median {median}ms, max {max}ms");
    }
    for (what, _, max) in &results[..3] {
        assert!(
            *max < VAULT_WRITE_BUDGET_MS,
            "{what} took {max}ms in the vault, over the {VAULT_WRITE_BUDGET_MS}ms budget"
        );
    }
    for (what, _, max) in &results[3..] {
        assert!(
            *max < VAULT_READ_BUDGET_MS,
            "{what} took {max}ms in the vault, over the {VAULT_READ_BUDGET_MS}ms budget"
        );
    }
}
