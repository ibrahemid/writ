//! The index the app holds: one writer connection, read connections beside
//! it, and the folder's names in memory.

use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, RwLock, RwLockReadGuard};

use rusqlite::Connection;
use writ_core::file_ops::THRESHOLD_NORMAL_BYTES;
use writ_core::notes::links::{self, Resolution};
use writ_core::search::{SearchHit, SearchQuery};
use writ_core::workspace::file_search::FileHit;

use super::queries;
use super::reconcile::{self, IndexAccess, RecentWrites, ReconcileOutcome};
use super::resolve::{NameIndex, NameSource};
use super::upsert::{self, Expect};
use super::walk::{is_dataless, should_index};
use super::writer::{WriteGuard, Writer};
use super::{
    index_key, schema, BacklinkRow, GraphRows, IndexedBy, IndexedNote, LinkRow, NoteFactsRow,
};
use crate::database::connection::{open_database, open_database_read_only};
use crate::errors::StorageResult;

/// Read connections kept open between commands. A command that finds none
/// idle opens another; one that finds the pool full closes its own.
const IDLE_READERS: usize = 4;

/// The notes index over connections of its own, the one the app holds.
///
/// Every write — the walk, the save path, the notes watcher — goes through
/// one writer connection, and holds it for one file: a walk reads a note with
/// nothing held, takes the writer to record that one note, and lets it go
/// before reading the next. Every read goes through a read connection of its
/// own. The database runs in WAL mode, where a reader sees the last committed
/// state without waiting for a writer, so quick open, link completion, the
/// backlink list and a link's resolution answer while a walk of thousands of
/// notes is under way. A walk takes the writer only while no other write is
/// waiting for it ([`Writer`]), so Cmd+N waits for at most the one write the
/// walk is making when it asks (ADR-036 section 7).
///
/// The folder's names are kept in memory ([`NameIndex`]) and changed by the
/// write that adds or removes a path, so a save resolves its links without
/// reading `files`. They follow writes made through another connection too:
/// every write that changes the set of paths moves a version kept in the
/// database ([`crate::schema_meta::KEY_NOTES_NAMES_VERSION`]), and the names
/// are read again when it differs from the one they were read at. A commit
/// that leaves the set alone, such as the buffer store stamping a save, costs
/// nothing here.
///
/// A write made through the store while a walk is under way is recorded, and
/// the walk leaves that path alone: it neither writes over it with what it
/// read before nor removes it at its end ([`RecentWrites`]).
pub struct NotesIndexStore {
    writer: Writer<Connection>,
    readers: ReaderPool,
    names: RwLock<NameCache>,
    recent_writes: Mutex<RecentWrites>,
    /// A store opened by another process, which only reads: its names are
    /// checked against the database before every use, because the app writes
    /// it without telling this one.
    read_only: bool,
    generation: AtomicU64,
}

/// The names, and the names version they match.
#[derive(Debug, Default)]
pub(crate) struct NameCache {
    index: NameIndex,
    /// The names version `index` matches, or `None` before the first read and
    /// after a write that changed `index` failed.
    version: Option<i64>,
    /// How many times `index` was read from the database.
    #[cfg(test)]
    loads: usize,
}

impl NameSource for RwLock<NameCache> {
    fn read<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> R {
        f(&recover(self.read()).index)
    }

    fn insert(&self, path: &str) -> bool {
        recover(self.write()).index.insert(path)
    }

    fn remove(&self, path: &str) -> bool {
        recover(self.write()).index.remove(path)
    }

    fn sync(&self, conn: &Connection) -> StorageResult<()> {
        let version = schema::names_version(conn)?;
        if recover(self.read()).version == Some(version) {
            return Ok(());
        }
        reload(self, conn)
    }

    fn committed(&self, version: i64) {
        recover(self.write()).version = Some(version);
    }

    fn abandon(&self) {
        recover(self.write()).version = None;
    }
}

/// Reads the names and the names version from `conn`, which the caller holds
/// inside a transaction so that both describe the same moment.
fn reload(cache: &RwLock<NameCache>, conn: &Connection) -> StorageResult<()> {
    let version = schema::names_version(conn)?;
    let index = NameIndex::load(conn)?;
    let mut cache = recover(cache.write());
    cache.index = index;
    cache.version = Some(version);
    #[cfg(test)]
    {
        cache.loads += 1;
    }
    Ok(())
}

/// A poisoned lock is recovered rather than cascaded: the index is derived
/// data that a walk rebuilds, so a panic while holding it costs at most a
/// stale row, and refusing every later search would cost the user the feature.
fn recover<G>(result: Result<G, std::sync::PoisonError<G>>) -> G {
    result.unwrap_or_else(|e| e.into_inner())
}

/// Read connections to one database, opened on demand and kept for the next
/// command.
struct ReaderPool {
    db_path: PathBuf,
    idle: Mutex<Vec<Connection>>,
}

impl ReaderPool {
    fn take(&self) -> StorageResult<PooledReader<'_>> {
        let held = recover(self.idle.lock()).pop();
        let conn = match held {
            Some(conn) => conn,
            None => open_database_read_only(&self.db_path)?,
        };
        Ok(PooledReader {
            conn: Some(conn),
            pool: self,
        })
    }
}

/// A read connection on loan from the pool, returned when dropped.
struct PooledReader<'a> {
    conn: Option<Connection>,
    pool: &'a ReaderPool,
}

impl Deref for PooledReader<'_> {
    type Target = Connection;

    fn deref(&self) -> &Connection {
        self.conn
            .as_ref()
            .expect("a pooled reader holds its connection until it is dropped")
    }
}

impl Drop for PooledReader<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut idle = recover(self.pool.idle.lock());
            if idle.len() < IDLE_READERS {
                idle.push(conn);
            }
        }
    }
}

impl NotesIndexStore {
    /// Opens the writer connection to the database at `db_path`. Read
    /// connections are opened as commands need them.
    ///
    /// Migrations are not run here. The primary connection runs them inside
    /// `AppState::initialize` before this one is opened, which is the same
    /// ordering the layout store relies on.
    pub fn open(db_path: &Path) -> StorageResult<Self> {
        Ok(Self::over(open_database(db_path)?, db_path, false))
    }

    /// Opens the database at `db_path` for reading only.
    ///
    /// For a process that is not the app: the `writ` command and the MCP
    /// server read the index this way so they can never create a database,
    /// run a migration or change a row. Every write method on this type fails
    /// on the connections it opens, which is the point (ADR-031 rule 1.3).
    ///
    /// An absent file is an error rather than an empty database. An existing
    /// one in WAL mode still gets its `-shm` and `-wal` companions created if
    /// they are not already there: SQLite needs the shared-memory index to read
    /// a WAL database at all, and it writes no frame into either.
    pub fn open_read_only(db_path: &Path) -> StorageResult<Self> {
        Ok(Self::over(open_database_read_only(db_path)?, db_path, true))
    }

    fn over(writer: Connection, db_path: &Path, read_only: bool) -> Self {
        Self {
            writer: Writer::new(writer),
            readers: ReaderPool {
                db_path: db_path.to_path_buf(),
                idle: Mutex::new(Vec::new()),
            },
            names: RwLock::new(NameCache::default()),
            recent_writes: Mutex::default(),
            read_only,
            generation: AtomicU64::new(0),
        }
    }

    /// The highest migration version the open database records. See
    /// [`crate::database::migrations::applied_schema_version`].
    pub fn schema_version(&self) -> StorageResult<i32> {
        self.read(crate::database::migrations::applied_schema_version)
    }

    /// Which folder the index is describing, as a number that changes whenever
    /// it changes.
    ///
    /// A walk takes seconds. If the notes folder moves while one is running,
    /// that walk finishes against the old folder and prunes every row it did
    /// not see, which is every row the move re-keyed. A walk therefore captures
    /// this value when it starts and gives up as soon as it differs.
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    /// Retires every walk in flight. Called before the rows move, so no walk
    /// started against the old folder can still be running when they do.
    pub fn bump_generation(&self) {
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// Re-keys the index from one notes folder to another. See
    /// [`super::NotesIndex::rekey_root`].
    ///
    /// Every path changes, so the names are read again from what the move
    /// left, and a walk that started before the move writes and removes
    /// nothing more: a row it wrote now would be keyed under the old folder.
    pub fn rekey_root(&self, from: &Path, to: &Path) -> StorageResult<usize> {
        let writer = self.writer();
        let changed = upsert::rekey(&writer, from, to)?;
        recover(self.recent_writes.lock()).moved_every_path();
        self.load_names(&writer)?;
        Ok(changed)
    }

    /// Up to `limit` ranked content hits. See [`super::NotesIndex::search_hits`].
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        self.search_hits_from(query, terms, 0, limit)
    }

    /// [`NotesIndexStore::search_hits`] after the first `offset` ranked hits,
    /// for a caller reading the hits a page at a time.
    pub fn search_hits_from(
        &self,
        query: &str,
        terms: &[String],
        offset: usize,
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        self.read(|conn| queries::search_hits(conn, query, terms, offset, limit))
    }

    /// Total number of notes matching `query`. See
    /// [`super::NotesIndex::count`].
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        self.read(|conn| queries::count(conn, query))
    }

    /// Up to `limit` ranked hits for a planned search after the first
    /// `offset`. See [`super::NotesIndex::search`].
    pub fn search(
        &self,
        query: &SearchQuery,
        offset: usize,
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        self.read(|conn| queries::search(conn, query, offset, limit))
    }

    /// Total number of notes a planned search matches. See
    /// [`super::NotesIndex::search_count`].
    pub fn search_count(&self, query: &SearchQuery) -> StorageResult<usize> {
        self.read(|conn| queries::search_count(conn, query))
    }

    /// Up to `limit` ranked name hits under `notes_root`, read from the names
    /// in memory. See [`super::NotesIndex::search_names`].
    pub fn search_names(
        &self,
        query: &str,
        notes_root: &Path,
        limit: usize,
    ) -> StorageResult<Vec<FileHit>> {
        self.with_names(|names| queries::search_names(names, query, notes_root, limit))
    }

    /// Every link written in the note at `path`. See
    /// [`super::NotesIndex::links_from`].
    pub fn links_from(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        self.read(|conn| queries::links_from(conn, path))
    }

    /// Every link that resolved to `path`. See
    /// [`super::NotesIndex::links_to`].
    pub fn links_to(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        self.read(|conn| queries::links_to(conn, path))
    }

    /// Every note a link could reach, in byte order, for a caller resolving
    /// links itself.
    ///
    /// A rename asks the same question of a link that the index asks: which
    /// note does this reach. Handing out the candidate list is what keeps the
    /// two answers the same one, so a rewrite never repoints a link the
    /// backlink list says belongs to another note.
    pub fn note_paths(&self) -> StorageResult<Vec<String>> {
        self.with_names(queries::note_paths)
    }

    /// The notes that link to `path`. See [`super::NotesIndex::backlinks`].
    ///
    /// The rows are read first and the names consulted after, so the names
    /// are held for the resolution of the ambiguous rows and nothing else.
    pub fn backlinks(&self, path: &str) -> StorageResult<Vec<BacklinkRow>> {
        let (resolved, unresolved) = self.read(|conn| queries::backlink_rows(conn, path))?;
        let ambiguous = self.with_names(|names| queries::ambiguous_for(names, path, unresolved))?;
        self.read(|conn| queries::describe_backlinks(conn, resolved, ambiguous))
    }

    /// Everything the index holds about `path`. See
    /// [`super::NotesIndex::facts`].
    pub fn facts(&self, path: &str) -> StorageResult<NoteFactsRow> {
        self.read(|conn| queries::facts(conn, path))
    }

    /// How much of the note at `path` the index holds. See
    /// [`super::NotesIndex::indexed_by`].
    pub fn indexed_by(&self, path: &str) -> StorageResult<Option<IndexedBy>> {
        self.read(|conn| queries::indexed_by(conn, path))
    }

    /// Every tag the index holds, with a note count each. See
    /// [`super::NotesIndex::all_tags`].
    pub fn all_tags(&self) -> StorageResult<Vec<(String, usize)>> {
        self.read(queries::all_tags)
    }

    /// Every note carrying one tag or a tag under it. See
    /// [`super::NotesIndex::paths_for_tag`].
    pub fn paths_for_tag(&self, tag: &str) -> StorageResult<Vec<String>> {
        self.read(|conn| queries::paths_for_tag(conn, tag))
    }

    /// Every note in the folder and the resolved links among them. See
    /// [`super::NotesIndex::graph`].
    pub fn graph(&self, notes_root: &Path) -> StorageResult<GraphRows> {
        self.read(|conn| queries::graph(conn, notes_root))
    }

    /// The notes a link naming `name` could mean. See
    /// [`super::NotesIndex::candidate_paths`].
    pub fn candidate_paths(&self, name: &str) -> StorageResult<Vec<String>> {
        self.with_names(|names| names.candidate_paths(name))
    }

    /// Resolves a link target. See [`super::NotesIndex::resolve_link`].
    pub fn resolve_link(&self, from_path: &str, target: &str) -> StorageResult<Resolution> {
        let parsed = links::parse_wikilink(target);
        self.with_names(|names| names.resolve(&parsed, from_path))
    }

    /// Resolves every target in `targets` as seen from the note at
    /// `from_path`, in order, against one check of the names: the wikilinks of
    /// a note a preview is about to render, asked for together rather than
    /// one query each (ADR-047).
    pub fn resolve_links(
        &self,
        from_path: &str,
        targets: &[links::WikilinkTarget],
    ) -> StorageResult<Vec<Resolution>> {
        self.with_names(|names| {
            targets
                .iter()
                .map(|target| names.resolve(target, from_path))
                .collect()
        })
    }

    /// Runs `f` with the index's writer held, so a walk waits for `f` at its
    /// next file rather than meeting a database `f` keeps busy: the startup
    /// maintenance, whose `VACUUM` holds the database's write lock for as long
    /// as it takes.
    pub fn holding_writes<R>(&self, f: impl FnOnce() -> R) -> R {
        let _writer = self.writer();
        f()
    }

    /// The line a heading anchor is on. See
    /// [`super::NotesIndex::heading_line`].
    pub fn heading_line(&self, path: &str, slug: &str) -> StorageResult<Option<u32>> {
        self.read(|conn| queries::heading_line(conn, path, slug))
    }

    /// Records `note` and its text, replacing whatever the index held for that
    /// path: the save path's write, and the watcher's once it has read the
    /// file.
    ///
    /// Holds the writer for this one note. A note already in the index costs
    /// its own rows and nothing else; a note new to it also fills in the
    /// links elsewhere that were waiting for it.
    pub fn upsert(&self, note: &IndexedNote, content: &str) -> StorageResult<()> {
        self.write_ahead(|conn, names| {
            upsert::write_file(conn, names, note, content, Expect::Anything)?;
            recover(self.recent_writes.lock()).wrote(&note.path);
            Ok(())
        })
    }

    /// Records the file at `path` in the index, reading its text.
    ///
    /// The watcher's create-or-modify arm. A file the filesystem reports as not
    /// downloaded, one over [`THRESHOLD_NORMAL_BYTES`], or one that is not note
    /// text is left alone rather than materialised or indexed as bytes; `false`
    /// says nothing was recorded. The file is read before the writer is taken.
    pub fn index_path(&self, path: &Path) -> StorageResult<bool> {
        if is_dataless(path) {
            return Ok(false);
        }
        let Some(note) = IndexedNote::from_file(path) else {
            return Ok(false);
        };
        if note.size > THRESHOLD_NORMAL_BYTES || !should_index(path) {
            return Ok(false);
        }
        let Ok(bytes) = std::fs::read(path) else {
            return Ok(false);
        };
        let Some(decoded) = writ_core::text_encoding::decode_file(&bytes) else {
            return Ok(false);
        };
        self.upsert(&note, &decoded.text)?;
        Ok(true)
    }

    /// Removes the file at `path` from the index.
    ///
    /// The watcher's delete arm, and the second half of a rename. Removing a
    /// path the index does not hold is not an error, which is what lets the
    /// subscriber replay an event without checking first.
    ///
    /// The links that pointed at the removed note lose their target, and every
    /// link naming it is resolved again, so a second note of the same name left
    /// standing picks them up rather than waiting for a walk.
    pub fn forget_path(&self, path: &Path) -> StorageResult<()> {
        self.forget_key(&index_key(path))
    }

    /// [`NotesIndexStore::forget_path`] for a row whose key the caller read
    /// while the file was still under it.
    ///
    /// After a rename that changes only case or normalisation, the old name
    /// still resolves on APFS and NTFS, to the file under its new spelling, so
    /// the key of the old row can no longer be read from the old path.
    pub fn forget_key(&self, key: &str) -> StorageResult<()> {
        self.write_ahead(|conn, names| {
            upsert::forget_file(conn, names, key)?;
            recover(self.recent_writes.lock()).wrote(key);
            Ok(())
        })
    }

    /// Walks `notes_root` and brings the index in line with it, holding the
    /// writer for one file at a time. See [`super::reconcile`].
    pub fn reconcile(
        &self,
        notes_root: &Path,
        cancelled: &dyn Fn() -> bool,
        is_dataless: &dyn Fn(&Path) -> bool,
    ) -> StorageResult<ReconcileOutcome> {
        reconcile::walk(self, notes_root, cancelled, is_dataless)
    }

    /// Every indexed path with its size and mtime. See
    /// [`super::NotesIndex::snapshot`].
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        self.read(schema::snapshot)
    }

    /// The writer, ahead of any walk.
    fn writer(&self) -> WriteGuard<'_, Connection> {
        self.writer.lock()
    }

    /// One write that is not a walk's: the writer, ahead of any walk, and the
    /// names, for `f` and no longer.
    fn write_ahead<R>(
        &self,
        f: impl FnOnce(&Connection, &RwLock<NameCache>) -> StorageResult<R>,
    ) -> StorageResult<R> {
        f(&self.writer(), &self.names)
    }

    fn names_cache(&self) -> RwLockReadGuard<'_, NameCache> {
        recover(self.names.read())
    }

    /// Reads the names from `files` through the writer, in one read
    /// transaction with the names version they stand at.
    fn load_names(&self, writer: &Connection) -> StorageResult<()> {
        let tx = writer.unchecked_transaction()?;
        reload(&self.names, &tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Reads the names again when another connection has changed the set of
    /// paths since they were read, or when they never were.
    fn sync_names(&self, writer: &Connection) -> StorageResult<()> {
        if self.names_cache().version == Some(schema::names_version(writer)?) {
            return Ok(());
        }
        let tx = writer.unchecked_transaction()?;
        self.names.sync(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Makes the names safe to read: read at least once, and not behind a
    /// commit some other connection made.
    ///
    /// A read in the app never waits for a write to check: when the writer is
    /// busy, the write holding it brings the names in line itself. A store
    /// that only reads has no write of its own to do that, so it checks every
    /// time.
    fn check_names(&self) -> StorageResult<()> {
        if self.read_only || self.names_cache().version.is_none() {
            return self.sync_names(&self.writer());
        }
        match self.writer.try_lock() {
            Some(writer) => self.sync_names(&writer),
            None => Ok(()),
        }
    }
}

impl IndexAccess for NotesIndexStore {
    type Names = RwLock<NameCache>;

    fn read<R>(&self, f: impl FnOnce(&Connection) -> StorageResult<R>) -> StorageResult<R> {
        let reader = self.readers.take()?;
        f(&reader)
    }

    fn write<R>(
        &self,
        f: impl FnOnce(&Connection, &Self::Names) -> StorageResult<R>,
    ) -> StorageResult<R> {
        f(&self.writer.lock_for_walk(), &self.names)
    }

    fn with_names<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> StorageResult<R> {
        self.check_names()?;
        Ok(f(&self.names_cache().index))
    }

    fn recent_writes(&self) -> &Mutex<RecentWrites> {
        &self.recent_writes
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use writ_core::buffer::manager::BufferManager;
    use writ_core::notes::links::Resolution;

    use super::NotesIndexStore;
    use crate::buffer_store::BufferStore;
    use crate::database::connection::open_database;
    use crate::database::migrations::run_migrations;
    use crate::notes_index::index_key;
    use crate::notes_index::reconcile::IndexAccess;

    /// The walk holds the writer for one file when Cmd+N's write arrives. The
    /// walk's next write finds the new note recorded: it let Cmd+N go first
    /// rather than taking the writer straight back.
    #[test]
    fn cmd_n_beside_a_walk_is_written_before_the_walks_next_file() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db_path = dir.path().join("writ.db");
        run_migrations(&open_database(&db_path).expect("open_database")).expect("migrations");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes dir");
        let fresh = notes.join("Fresh.md");
        std::fs::write(&fresh, "# Fresh\n").expect("write note");
        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));

        let walk_file = index.writer.lock_for_walk();
        let cmd_n = {
            let index = Arc::clone(&index);
            let fresh = fresh.clone();
            std::thread::spawn(move || index.index_path(&fresh))
        };
        while index.writer.ahead() == 0 {
            std::thread::yield_now();
        }
        drop(walk_file);
        let recorded: i64 = IndexAccess::write(&*index, |conn, _| {
            Ok(conn.query_row(
                "SELECT COUNT(*) FROM files WHERE path = ?1",
                [index_key(&fresh)],
                |row| row.get(0),
            )?)
        })
        .expect("the walk's next write");

        assert!(cmd_n.join().expect("Cmd+N thread").expect("index_path"));
        assert_eq!(recorded, 1, "the walk wrote again before Cmd+N's write");
    }

    /// With the writer held part way through a write that has not committed,
    /// the way a walk holds it for one file, quick open, search, backlinks, a
    /// link's resolution and the graph all answer from the last commit without
    /// waiting for the write to end (ADR-036 section 7).
    #[test]
    fn reads_answer_from_the_last_commit_while_the_writer_is_held() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db_path = dir.path().join("writ.db");
        run_migrations(&open_database(&db_path).expect("open_database")).expect("migrations");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes dir");
        for idx in 0..3 {
            std::fs::write(
                notes.join(format!("note-{idx}.md")),
                format!("# Note {idx}\n\nkestrel and [[note-{}]]\n", (idx + 1) % 3),
            )
            .expect("write note");
        }
        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        index
            .reconcile(&notes, &|| false, &|_| false)
            .expect("reconcile");
        let linked = index_key(&notes.join("note-1.md"));
        let linking = index_key(&notes.join("note-0.md"));
        assert_eq!(
            index.resolve_link(&linking, "note-1").expect("resolve"),
            Resolution::Resolved(linked.clone())
        );

        let writer = index.writer.lock_for_walk();
        writer
            .execute_batch("BEGIN IMMEDIATE; DELETE FROM links; DELETE FROM files;")
            .expect("a write under way");
        let (tx, rx) = std::sync::mpsc::channel();
        {
            let index = Arc::clone(&index);
            let notes = notes.clone();
            let (linked, linking) = (linked.clone(), linking.clone());
            std::thread::spawn(move || {
                let answer = (|| -> crate::errors::StorageResult<_> {
                    let query = writ_core::search::to_prefix_match("kestrel").expect("query");
                    let terms = writ_core::search::search_terms("kestrel");
                    Ok((
                        index.search_names("note", &notes, 10)?.len(),
                        index.search_hits(&query, &terms, 10)?.len(),
                        index.backlinks(&linked)?.len(),
                        index.resolve_link(&linking, "note-1")?,
                        index.graph(&notes)?,
                    ))
                })();
                let _ = tx.send(answer);
            });
        }
        let answer = rx.recv_timeout(std::time::Duration::from_secs(10));
        writer.execute_batch("ROLLBACK").expect("rollback");
        drop(writer);

        let (names, hits, backlinks, resolved, graph) = answer
            .expect("a read waited for the writer")
            .expect("the reads");
        assert_eq!(names, 3);
        assert_eq!(hits, 3);
        assert_eq!(backlinks, 1);
        assert_eq!(resolved, Resolution::Resolved(linked));
        assert_eq!((graph.nodes.len(), graph.edges.len()), (3, 3));
    }

    /// The app's save: the buffer store writes the file and stamps its row
    /// through a connection of its own, then the deferred reindex writes the
    /// note through the index, and quick open and the preview's link
    /// resolution read between saves. None of that adds or removes a note, so
    /// none of it reads the folder's names again (RC-14).
    #[test]
    fn saves_through_the_buffer_store_never_read_the_names_again() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let db_path = dir.path().join("writ.db");
        let conn = open_database(&db_path).expect("open_database");
        run_migrations(&conn).expect("migrations");
        let notes = dir.path().join("notes");
        std::fs::create_dir_all(&notes).expect("notes dir");
        for idx in 0..50 {
            std::fs::write(
                notes.join(format!("note-{idx:02}.md")),
                format!("# Note {idx}\n\n[[note-{:02}]]\n", (idx + 1) % 50),
            )
            .expect("write note");
        }
        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        index
            .reconcile(&notes, &|| false, &|_| false)
            .expect("reconcile");

        let buffers = dir.path().join("buffers");
        std::fs::create_dir_all(&buffers).expect("buffers dir");
        let mut store = BufferStore::new(conn, buffers);
        store.set_notes_root(notes.clone());
        store.set_notes_index(Arc::clone(&index));
        let doc = BufferManager::new()
            .create_buffer(Some("note-07".to_string()))
            .expect("mint");
        store.insert(&doc).expect("insert");
        let path = notes.join("note-07.md");
        store
            .attach_source_path(&doc.id, path.to_str().expect("path"))
            .expect("attach");
        let from = index_key(&notes.join("note-03.md"));
        let resolves = |index: &NotesIndexStore| {
            assert!(matches!(
                index.resolve_link(&from, "note-07").expect("resolve"),
                Resolution::Resolved(_)
            ));
        };
        resolves(&index);
        let loads = index.names_cache().loads;

        let mut state = None;
        for turn in 0..5 {
            state = Some(
                store
                    .save_to_source_without_index(
                        &doc.id,
                        &format!("# Note 7\n\nedit {turn} [[note-08]]\n"),
                        state,
                        None,
                    )
                    .expect("save"),
            );
            store.reindex_buffer(&doc.id).expect("reindex");
            assert!(!index
                .search_names("note-0", Path::new(&notes), 10)
                .expect("names")
                .is_empty());
            resolves(&index);
        }

        assert_eq!(
            index.names_cache().loads,
            loads,
            "a save that adds no note read the folder's names again"
        );
    }
}
