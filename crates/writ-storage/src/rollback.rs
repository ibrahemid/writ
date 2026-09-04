//! The copy of the database taken before the notes migration writes anything.
//!
//! ADR-028 §4: before the first write of the notes migration, `writ.db` is
//! copied beside itself as `writ.db.pre-notes-migration` and the copy is
//! recorded in [`crate::schema_meta`]. The copy is kept for ten launches and
//! then deleted.
//!
//! The copy is taken with `VACUUM INTO`, which writes one self-contained
//! file holding every committed page, including the pages still in the
//! write-ahead log. Copying the three files instead does not work: after a
//! clean shutdown the database file is a bare header and every page sits in
//! the log, so a copied `.db` restored on its own opens empty; and after an
//! unclean exit SQLite replays the live post-migration log onto the restored
//! pre-migration file, so the restore silently does nothing.
//!
//! Restoring the copy is therefore three steps: close the database, delete
//! `writ.db-wal` and `writ.db-shm`, then rename the copy over `writ.db`.
//! Deleting the two log files is not optional, for the reason above, and the
//! restore surface must do all three.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};

use crate::errors::{StorageError, StorageResult};
use crate::schema_meta::{self, KEY_ROLLBACK_COPY_LAUNCHES, KEY_ROLLBACK_COPY_PATH};

/// Ten launches (ADR-028 §4).
pub const ROLLBACK_KEEP_LAUNCHES: u32 = 10;

/// What the copy appends to the name of the database it was taken from.
pub const ROLLBACK_COPY_SUFFIX: &str = ".pre-notes-migration";

/// Copies the database `conn` is open on to `<db_path>.pre-notes-migration`
/// beside it and records the copy in `schema_meta`, stamped with the RFC 3339
/// time of the write.
///
/// `db_path` names the file `conn` is open on: the copy is written through
/// `conn`, and `db_path` only supplies the name it is written under.
///
/// Writing the copy is a no-op when it already exists: a second call over an
/// existing copy keeps the copy, its recorded path and its launch count
/// untouched, so a re-run of the migration cannot overwrite the state the
/// copy was taken to preserve.
///
/// The launch counter starts at 0, so the launch that took the copy counts
/// itself: the caller runs [`age_out_rollback_copy`] on every launch,
/// including this one.
pub fn write_rollback_copy(conn: &Connection, db_path: &Path) -> StorageResult<PathBuf> {
    // `VACUUM INTO` cannot run inside a transaction, and a copy taken from a
    // connection with uncommitted work would not hold that work anyway.
    if !conn.is_autocommit() {
        return Err(StorageError::RollbackCopyInTransaction);
    }

    let copy = rollback_copy_path(db_path);
    let recorded_path = path_as_text(&copy)?;

    let existed = copy.exists();
    if !existed {
        conn.execute("VACUUM INTO ?1", params![&recorded_path])
            .map_err(|cause| {
                // A half-written copy is worse than none: the next launch
                // would see the file, take it for a good copy, and skip
                // taking a real one.
                let _ = remove_file(&copy);
                StorageError::RollbackCopyWrite {
                    path: copy.clone(),
                    cause,
                }
            })?;
    }

    if !existed || schema_meta::get(conn, KEY_ROLLBACK_COPY_PATH)?.is_none() {
        schema_meta::set(conn, KEY_ROLLBACK_COPY_PATH, &recorded_path)?;
        schema_meta::set(conn, KEY_ROLLBACK_COPY_LAUNCHES, "0")?;
    }

    Ok(copy)
}

/// Counts one launch against the recorded copy and deletes it once it has
/// survived `keep_launches` of them, clearing both `schema_meta` rows.
///
/// Returns `true` on the launch that deleted the copy. A run with no recorded
/// copy counts nothing and returns `false`. The caller runs this on every
/// launch, including the one that wrote the copy, and treats a failure as a
/// line in the log rather than a reason to stop starting up: the count is
/// bookkeeping, and a launch that cannot keep it still has to open.
pub fn age_out_rollback_copy(conn: &Connection, keep_launches: u32) -> StorageResult<bool> {
    let Some(recorded) = schema_meta::get(conn, KEY_ROLLBACK_COPY_PATH)? else {
        return Ok(false);
    };

    let launches = read_launches(conn)?.saturating_add(1);
    if launches < keep_launches {
        schema_meta::set(conn, KEY_ROLLBACK_COPY_LAUNCHES, &launches.to_string())?;
        return Ok(false);
    }

    remove_file(Path::new(&recorded))?;
    schema_meta::clear(conn, KEY_ROLLBACK_COPY_PATH)?;
    schema_meta::clear(conn, KEY_ROLLBACK_COPY_LAUNCHES)?;
    Ok(true)
}

/// Where the copy of `db_path` lives.
fn rollback_copy_path(db_path: &Path) -> PathBuf {
    let mut name = db_path.as_os_str().to_owned();
    name.push(ROLLBACK_COPY_SUFFIX);
    PathBuf::from(name)
}

/// How many launches the recorded copy has survived so far.
fn read_launches(conn: &Connection) -> StorageResult<u32> {
    let Some(value) = schema_meta::get(conn, KEY_ROLLBACK_COPY_LAUNCHES)? else {
        return Ok(0);
    };
    value
        .trim()
        .parse::<u32>()
        .map_err(|_| StorageError::SchemaMetaValue {
            key: KEY_ROLLBACK_COPY_LAUNCHES.to_string(),
            value,
        })
}

fn remove_file(path: &Path) -> StorageResult<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(cause) => Err(StorageError::RollbackCopyRemove {
            path: path.to_path_buf(),
            cause,
        }),
    }
}

/// A path as the text a `schema_meta` row holds. A path that would not
/// survive the round trip back to a path is an error, not a lossy string.
fn path_as_text(path: &Path) -> StorageResult<String> {
    path.to_str()
        .map(str::to_string)
        .ok_or_else(|| StorageError::UnrecordablePath {
            path: path.to_path_buf(),
        })
}
