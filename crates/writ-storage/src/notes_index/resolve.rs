//! The names a link can mean, kept in memory, and the passes that point
//! stored links at the notes they name.

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use rusqlite::{params, Connection};
use writ_core::notes::links::{self, NameCandidate, Resolution, WikilinkTarget};

use crate::errors::StorageResult;

/// Every indexed path, grouped by the names a link can call it by.
///
/// Resolution compares a target against every note that shares its name, so
/// grouping the folder's paths here once is what keeps a walk, a save and a
/// backlink list from reading `files` for every link they resolve. The group
/// keys come from [`links::candidate_name_keys`], the same function the
/// resolver matches with, so the grouping can never hide a candidate the
/// resolver would have accepted. Each path is folded once, when it is added
/// ([`NameCandidate`]), and never again while it stays.
///
/// [`super::NotesIndexStore`] keeps one of these in memory for the life of
/// the app and changes it in the same write that adds or removes a path, so a
/// save resolves its links without reading the folder's names (EP-16).
#[derive(Debug, Default, Clone)]
pub(crate) struct NameIndex {
    by_key: HashMap<String, Vec<Arc<NameCandidate>>>,
    by_path: HashMap<String, Arc<NameCandidate>>,
}

impl NameIndex {
    /// Every path `files` holds, read through `conn`.
    pub(crate) fn load(conn: &Connection) -> StorageResult<Self> {
        let mut stmt = conn.prepare("SELECT path FROM files")?;
        let mut index = Self::default();
        for path in stmt.query_map([], |row| row.get::<_, String>(0))? {
            index.insert(&path?);
        }
        Ok(index)
    }

    /// Records `path` under every name it answers to. `false` when it was
    /// already held.
    pub(crate) fn insert(&mut self, path: &str) -> bool {
        if self.by_path.contains_key(path) {
            return false;
        }
        let candidate = Arc::new(NameCandidate::new(path));
        for key in candidate.keys() {
            self.by_key
                .entry(key.clone())
                .or_default()
                .push(Arc::clone(&candidate));
        }
        self.by_path.insert(path.to_string(), candidate);
        true
    }

    /// Forgets `path`. `false` when it was not held.
    pub(crate) fn remove(&mut self, path: &str) -> bool {
        let Some(candidate) = self.by_path.remove(path) else {
            return false;
        };
        for key in candidate.keys() {
            if let Some(group) = self.by_key.get_mut(key) {
                group.retain(|held| held.path() != path);
                if group.is_empty() {
                    self.by_key.remove(key);
                }
            }
        }
        true
    }

    /// The indexed notes a link naming `name` could mean.
    fn candidates<'s>(&'s self, name: &str) -> impl Iterator<Item = &'s NameCandidate> + 's {
        self.by_key
            .get(&links::name_key(name))
            .into_iter()
            .flatten()
            .map(|candidate| candidate.as_ref())
    }

    /// The indexed paths a link naming `name` could mean, in byte order.
    pub(crate) fn candidate_paths(&self, name: &str) -> Vec<String> {
        let mut found: Vec<String> = self
            .candidates(name)
            .map(|candidate| candidate.path().to_string())
            .collect();
        found.sort_unstable();
        found
    }

    /// Resolves `target` as seen from the note at `from`.
    pub(crate) fn resolve(&self, target: &WikilinkTarget, from: &str) -> Resolution {
        links::resolve_candidates(target, from, self.candidates(&target.name))
    }

    /// Every held path, in no particular order.
    pub(crate) fn paths(&self) -> impl Iterator<Item = &str> {
        self.by_path.keys().map(String::as_str)
    }
}

/// Where a write finds the names it resolves against, and where it records
/// the paths it adds and removes.
///
/// A walk over one borrowed connection keeps its names in a [`RefCell`], read
/// from that connection when the walk starts; the store keeps the app's in a
/// lock that a reader only ever holds for one lookup, and brings them in line
/// with the database at the start of every write.
pub(crate) trait NameSource {
    /// Runs `f` over the names as they stand.
    fn read<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> R;
    /// Records `path`. `false` when it was already held.
    fn insert(&self, path: &str) -> bool;
    /// Forgets `path`. `false` when it was not held.
    fn remove(&self, path: &str) -> bool;
    /// Brings the names in line with what `conn` sees, inside a transaction
    /// that holds the database's write lock.
    fn sync(&self, conn: &Connection) -> StorageResult<()>;
    /// The write that changed the names committed, leaving the names version
    /// at `version`.
    fn committed(&self, version: i64);
    /// A write that may have changed the names failed, so they may hold a
    /// path the database does not.
    fn abandon(&self);
}

impl NameSource for RefCell<NameIndex> {
    fn read<R>(&self, f: impl FnOnce(&NameIndex) -> R) -> R {
        f(&self.borrow())
    }

    fn insert(&self, path: &str) -> bool {
        self.borrow_mut().insert(path)
    }

    fn remove(&self, path: &str) -> bool {
        self.borrow_mut().remove(path)
    }

    fn sync(&self, _conn: &Connection) -> StorageResult<()> {
        Ok(())
    }

    fn committed(&self, _version: i64) {}

    fn abandon(&self) {}
}

/// The path a resolution stores in `links.to_path`: the note it reached, or
/// `NULL` for a link that reached none or more than one.
///
/// An ambiguous target is not a link to the alphabetically first candidate
/// (ADR-034), and a target with no note behind it yet is filled in when that
/// note arrives.
pub(crate) fn stored_path(resolution: Resolution) -> Option<String> {
    match resolution {
        Resolution::Resolved(path) => Some(path),
        Resolution::Ambiguous(_) | Resolution::Missing => None,
    }
}

/// Recomputes `to_path` for every link that names one of `keys`. Returns the
/// number of links whose target changed.
///
/// A note arriving or leaving can only change the links that name it, so this
/// is the whole of what an added or removed path costs: one indexed lookup on
/// `links.name_key` per key (migration 044), rather than a read of every link
/// in the folder (RC-14).
///
/// Every visited row is resolved from scratch and written back, including
/// back to `NULL`. A link is stored with no target both when the note it
/// names does not exist yet and when two notes answer to that name, and either
/// can become the other: a note arriving fills in the links that waited for
/// it, a second note of the same name makes an answered link ambiguous again,
/// and a note leaving empties the links that reached it.
///
/// `to_target` was stored from a target the scanner had already parsed, so it
/// is read back through [`links::stored_target`] and never parsed a second
/// time: `[[Note.md.md]]` must not lose another extension on the way back.
pub(crate) fn resolve_links_named(
    conn: &Connection,
    names: &NameIndex,
    keys: &[String],
) -> StorageResult<usize> {
    let mut changed = 0usize;
    for (at, key) in keys.iter().enumerate() {
        if keys[..at].contains(key) {
            continue;
        }
        let rows: Vec<(i64, String, String, Option<String>)> = {
            let mut stmt = conn.prepare_cached(
                "SELECT rowid, from_path, to_target, to_path FROM links WHERE name_key = ?1",
            )?;
            let rows = stmt.query_map(params![key], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })?;
            rows.collect::<Result<Vec<_>, _>>()?
        };
        for (rowid, from_path, to_target, was) in rows {
            let now = stored_path(names.resolve(&links::stored_target(&to_target), &from_path));
            if now == was {
                continue;
            }
            conn.prepare_cached("UPDATE links SET to_path = ?2 WHERE rowid = ?1")?
                .execute(params![rowid, now])?;
            changed += 1;
        }
    }
    Ok(changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn index(paths: &[&str]) -> NameIndex {
        let mut index = NameIndex::default();
        for path in paths {
            index.insert(path);
        }
        index
    }

    #[test]
    fn a_path_is_held_once_under_each_of_its_names() {
        let mut names = index(&["/n/Note.md"]);
        assert!(!names.insert("/n/Note.md"));
        assert_eq!(names.candidate_paths("note"), vec!["/n/Note.md"]);
        assert_eq!(names.candidate_paths("Note.md"), vec!["/n/Note.md"]);
        assert_eq!(names.paths().count(), 1);
    }

    #[test]
    fn removing_a_path_takes_it_out_of_every_group() {
        let mut names = index(&["/n/a/Note.md", "/n/b/Note.md"]);
        assert!(names.remove("/n/a/Note.md"));
        assert!(!names.remove("/n/a/Note.md"));
        assert_eq!(names.candidate_paths("Note"), vec!["/n/b/Note.md"]);
        assert!(names.remove("/n/b/Note.md"));
        assert!(names.candidate_paths("Note").is_empty());
        assert!(names.by_key.is_empty(), "an emptied group is dropped");
    }

    #[test]
    fn resolution_follows_the_names_as_they_change() {
        let mut names = index(&["/n/a/Note.md"]);
        let target = links::parse_target("Note");
        assert_eq!(
            names.resolve(&target, "/n/c/From.md"),
            Resolution::Resolved("/n/a/Note.md".to_string())
        );
        names.insert("/n/b/Note.md");
        assert!(matches!(
            names.resolve(&target, "/n/c/From.md"),
            Resolution::Ambiguous(_)
        ));
        names.remove("/n/a/Note.md");
        assert_eq!(
            names.resolve(&target, "/n/c/From.md"),
            Resolution::Resolved("/n/b/Note.md".to_string())
        );
    }
}
