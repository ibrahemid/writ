use crate::errors::StorageResult;
use rusqlite::{Connection, OpenFlags};
use std::cell::Cell;
use std::path::Path;
use std::time::{Duration, Instant};

/// Upper bound, in bytes, that the write-ahead log is truncated to on
/// checkpoint.
const JOURNAL_SIZE_LIMIT_BYTES: i64 = 64 * 1024 * 1024;

/// How long a write the database refused waits before it is tried again.
const BUSY_RETRY_INTERVAL: Duration = Duration::from_millis(1);

/// How long a refused write keeps being tried again before the refusal
/// reaches the caller: the five seconds SQLite's own timeout allowed.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

thread_local! {
    /// When the write this thread is trying again was first refused. SQLite
    /// calls the busy handler on the thread making the write, counting the
    /// tries from zero for each write.
    static REFUSED_AT: Cell<Option<Instant>> = const { Cell::new(None) };
}

/// Opens a SQLite database at `path` using Writ's pragma defaults.
///
/// The returned connection has:
///
/// - `journal_mode = WAL` for concurrent read/write performance,
/// - `synchronous = NORMAL` for durability within a transaction while
///   avoiding the fsync cost of `FULL` on every commit,
/// - `foreign_keys = ON` so referential constraints are enforced,
/// - `journal_size_limit = 64 MiB` so a checkpoint truncates the
///   write-ahead log back to that cap instead of leaving it at its
///   high-water mark,
/// - a write refused because another connection holds the write lock tried
///   again every millisecond, for up to five seconds.
pub fn open_database(path: &Path) -> StorageResult<Connection> {
    let conn = Connection::open(path)?;
    conn.busy_handler(Some(retry_every_millisecond))?;
    conn.pragma_update(None, "journal_mode", "wal")?;
    conn.pragma_update(None, "synchronous", "normal")?;
    conn.pragma_update(None, "foreign_keys", "on")?;
    conn.pragma_update(None, "journal_size_limit", JOURNAL_SIZE_LIMIT_BYTES)?;
    Ok(conn)
}

/// Waits before a refused write is tried again, `attempts` refusals in;
/// `false` hands the refusal to the caller.
///
/// SQLite's own handler waits longer after every refusal, up to 100 ms between
/// tries. The notes walk takes the write lock once per file, a millisecond or
/// so at a time, on a connection of its own, so a write on the app's connection
/// that waits 25 or 100 ms between tries keeps waking up in the middle of a
/// later file: Cmd+N's row waited 295 ms while a walk went on through 293 more
/// entries. Trying every millisecond finds the moment between two files.
fn retry_every_millisecond(attempts: i32) -> bool {
    #[cfg(test)]
    tests::retried_on_this_thread();
    let now = Instant::now();
    let Ok(refused_at) = REFUSED_AT.try_with(|refused_at| match refused_at.get() {
        Some(at) if attempts > 0 => at,
        _ => {
            refused_at.set(Some(now));
            now
        }
    }) else {
        return false;
    };
    if now.duration_since(refused_at) >= BUSY_TIMEOUT {
        return false;
    }
    std::thread::sleep(BUSY_RETRY_INTERVAL);
    true
}

/// Opens the database at `path` for reading only.
///
/// For a second process that must not change what the app owns: the `writ`
/// command reads the note index this way. No pragma is set, because every one
/// of the defaults above is a write — `journal_mode` in particular — and a
/// read-only connection refuses them. An absent file is an error here rather
/// than a fresh empty database, which is what [`open_database`] would create.
pub fn open_database_read_only(path: &Path) -> StorageResult<Connection> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}

#[cfg(test)]
mod tests {
    use std::sync::{Mutex, PoisonError};
    use std::thread::ThreadId;
    use std::time::{Duration, Instant};

    use super::{
        open_database, retry_every_millisecond, BUSY_RETRY_INTERVAL, BUSY_TIMEOUT, REFUSED_AT,
    };

    /// The threads [`retry_every_millisecond`] has run on in this test binary.
    static RETRIED_ON: Mutex<Vec<ThreadId>> = Mutex::new(Vec::new());

    pub(super) fn retried_on_this_thread() {
        let id = std::thread::current().id();
        let mut threads = RETRIED_ON.lock().unwrap_or_else(PoisonError::into_inner);
        if !threads.contains(&id) {
            threads.push(id);
        }
    }

    fn retried_on(id: ThreadId) -> bool {
        RETRIED_ON
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&id)
    }

    /// Each try waits a millisecond first, and the tries stop five seconds
    /// after the write was first refused, however long each wait really took.
    #[test]
    fn a_refused_write_is_tried_every_millisecond_for_five_seconds() {
        let start = Instant::now();
        assert!(retry_every_millisecond(0));
        assert!(start.elapsed() >= BUSY_RETRY_INTERVAL);
        assert!(retry_every_millisecond(1));

        let long_ago = Instant::now()
            .checked_sub(BUSY_TIMEOUT)
            .expect("the clock reaches five seconds back");
        REFUSED_AT.with(|refused_at| refused_at.set(Some(long_ago)));
        assert!(
            !retry_every_millisecond(2),
            "five seconds after the first refusal"
        );
        assert!(
            retry_every_millisecond(0),
            "a new write's first refusal starts its own five seconds"
        );
    }

    /// A write on one connection while another holds the write lock is tried
    /// again through Writ's handler, and lands once the lock is let go.
    #[test]
    fn a_write_another_connection_holds_up_is_retried_until_it_lands() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let path = dir.path().join("writ.db");
        let holder = open_database(&path).expect("holder");
        holder
            .execute_batch(
                "CREATE TABLE notes (id INTEGER); BEGIN IMMEDIATE; INSERT INTO notes VALUES (0);",
            )
            .expect("hold the write lock");
        let writer = std::thread::spawn({
            let path = path.clone();
            move || {
                open_database(&path)
                    .expect("writer")
                    .execute("INSERT INTO notes VALUES (1)", [])
            }
        });
        let writer_thread = writer.thread().id();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !retried_on(writer_thread) {
            assert!(
                Instant::now() < deadline,
                "the refused write was never tried again through Writ's handler"
            );
            std::thread::yield_now();
        }
        holder.execute_batch("COMMIT").expect("let the lock go");

        writer
            .join()
            .expect("writer thread")
            .expect("the write lands once the lock is let go");
        let rows: i64 = holder
            .query_row("SELECT COUNT(*) FROM notes", [], |row| row.get(0))
            .expect("count");
        assert_eq!(rows, 2);
    }
}
