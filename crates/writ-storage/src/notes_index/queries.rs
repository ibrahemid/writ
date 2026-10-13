//! The reads.
//!
//! Each function takes the connection it reads through and, where a link has
//! to be resolved, the names it resolves against. [`NotesIndex`] reads the
//! names from its own connection per call; the store hands over the ones it
//! keeps in memory and reads through a connection no write holds.

use std::collections::HashSet;
use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use writ_core::notes::links::{self, Resolution};
use writ_core::notes::{self, snippet};
use writ_core::search::{build_hit, MarkedFragment, SearchHit};
use writ_core::workspace::file_search::{rank_keyed_file_hits, FileHit};

use super::resolve::NameIndex;
use super::{
    file_name_of, folder_segment, is_note_file, root_prefix, schema, BacklinkCertainty,
    BacklinkRow, GraphEdge, GraphNode, GraphRows, HeadingRow, IndexedBy, LinkRow, NoteFactsRow,
    NotesIndex,
};
use crate::errors::StorageResult;

/// How many tokens of a note the full-text index hands back around a match.
/// Enough for the line a match sits on, which is all a result shows.
const SNIPPET_TOKENS: i64 = 32;

/// Up to `limit` ranked content hits, each carrying the note's path, name,
/// matching line and a highlighted snippet.
///
/// The snippet is cut by the index itself (`snippet()`), which marks the
/// matches with the tokenizer that found them, so the text of a note never
/// leaves the index to be shown (RC-34). The line the first match is on is
/// worked out inside the index as well, from where the fragment sits in the
/// note ([`match_line`]).
///
/// The marks are control characters, and a note can hold them too. Its own
/// cannot be told apart from the index's, so a fragment holding one is not a
/// run of the note's text and is found nowhere in it. That note's hit is read
/// from its text instead ([`build_hit`]), the way every hit was read before
/// the index cut fragments.
pub(crate) fn search_hits(
    conn: &Connection,
    query: &str,
    terms: &[String],
    limit: usize,
) -> StorageResult<Vec<SearchHit>> {
    let rows: Vec<(i64, String, String, String)> = {
        let mut stmt = conn.prepare_cached(
            "SELECT x.rowid, f.path, x.name,
                    snippet(files_fts, 1, char(2), char(3), char(4), ?3)
               FROM files_fts x
               JOIN files f ON f.rowid = x.rowid
              WHERE files_fts MATCH ?1
              ORDER BY rank
              LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64, SNIPPET_TOKENS], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };

    let mut hits = Vec::with_capacity(rows.len());
    for (rowid, path, name, marked) in rows {
        let fragment = MarkedFragment::read(&marked);
        // A note has no buffer id until something opens it. The adapter
        // fills one in for a note that is already a tab; the path is what
        // opens the rest.
        let mut hit = match fragment.first_match() {
            Some(offset) => match match_line(conn, rowid, &fragment.text, offset)? {
                Some(line) => fragment.into_hit(&name, Some(line), terms),
                None => build_hit("", &name, &note_text(conn, &path)?, terms),
            },
            None => fragment.into_hit(&name, None, terms),
        };
        hit.path = Some(path);
        hits.push(hit);
    }
    Ok(hits)
}

/// The 1-based line of a note that a fragment's first match is on, or `None`
/// when the fragment cannot be found in the note's text.
///
/// `fragment` is a run of the note's text exactly as it stands, and `offset`
/// the characters before the first match inside it, so the line is one more
/// than the line breaks before that point. SQLite counts all three in
/// characters, and the count happens where the text already is.
fn match_line(
    conn: &Connection,
    rowid: i64,
    fragment: &str,
    offset: usize,
) -> StorageResult<Option<u32>> {
    let line: Option<Option<i64>> = conn
        .prepare_cached(
            "SELECT CASE WHEN at = 0 THEN NULL
                         ELSE 1 + length(substr(content, 1, at - 1 + ?3))
                                - length(replace(substr(content, 1, at - 1 + ?3), char(10), ''))
                    END
               FROM (SELECT content, instr(content, ?2) AS at FROM files_fts WHERE rowid = ?1)",
        )?
        .query_row(params![rowid, fragment, offset as i64], |row| row.get(0))
        .optional()?;
    Ok(line.flatten().map(|line| line as u32))
}

/// Total number of notes matching `query`, independent of any result limit.
pub(crate) fn count(conn: &Connection, query: &str) -> StorageResult<usize> {
    let total: i64 = conn.query_row(
        "SELECT count(*) FROM files_fts WHERE files_fts MATCH ?1",
        params![query],
        |row| row.get(0),
    )?;
    Ok(total as usize)
}

/// Up to `limit` ranked name-only hits, for quick open and `[[` completion.
///
/// Ranked by the same subsequence scorer the workspace file palette uses, so
/// a prefix of a note's name outranks a match in the middle of one and the
/// two surfaces order their results the same way.
///
/// A note is findable by its name and by where it sits inside `notes_root`,
/// never by the folders above it: rows are keyed by absolute path, so ranking
/// that key would let a query reach every note through the letters of the home
/// directory. What is ranked is the path under the root, spelled with forward
/// slashes so the order two notes come back in is the same on every platform,
/// and each hit still carries the index key the caller opens. A row outside
/// the root (a folder that moved while the walk was retired) is ranked on its
/// name alone.
///
/// The candidates are the names the index already holds in memory, so a
/// keystroke reads no table (RC-34).
pub(crate) fn search_names(
    names: &NameIndex,
    query: &str,
    notes_root: &Path,
    limit: usize,
) -> Vec<FileHit> {
    let prefix = root_prefix(notes_root);
    let candidates: Vec<(&str, String, &str)> = names
        .paths()
        .map(|path| {
            let name = file_name_of(path);
            let relative = match path.strip_prefix(&prefix) {
                Some(rest) => rest.replace('\\', "/"),
                None => name.to_string(),
            };
            (path, relative, name)
        })
        .collect();

    rank_keyed_file_hits(
        query,
        candidates
            .iter()
            .map(|(path, relative, name)| (*path, relative.as_str(), *name)),
        limit,
    )
}

/// Every indexed path, in byte order.
pub(crate) fn note_paths(names: &NameIndex) -> Vec<String> {
    let mut paths: Vec<String> = names.paths().map(str::to_string).collect();
    paths.sort_unstable();
    paths
}

/// Every link written in the note at `path`.
pub(crate) fn links_from(conn: &Connection, path: &str) -> StorageResult<Vec<LinkRow>> {
    link_rows(
        conn,
        "SELECT from_path, to_target, to_path, kind, line, col FROM links
          WHERE from_path = ?1 ORDER BY line, col",
        path,
    )
}

/// Every link that resolved to the note at `path`, through the index on
/// `links(to_path)`.
pub(crate) fn links_to(conn: &Connection, path: &str) -> StorageResult<Vec<LinkRow>> {
    link_rows(
        conn,
        "SELECT from_path, to_target, to_path, kind, line, col FROM links
          WHERE to_path = ?1 ORDER BY from_path, line, col",
        path,
    )
}

/// Runs a one-parameter query over `links`.
fn link_rows(conn: &Connection, sql: &str, value: &str) -> StorageResult<Vec<LinkRow>> {
    let mut stmt = conn.prepare_cached(sql)?;
    let rows = stmt
        .query_map(params![value], |row| {
            Ok(LinkRow {
                from_path: row.get(0)?,
                to_target: row.get(1)?,
                to_path: row.get(2)?,
                kind: row.get(3)?,
                line: row.get::<_, i64>(4)? as u32,
                col: row.get::<_, i64>(5)? as u32,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The notes that link to the note at `path`, each link with the sentence it
/// sits in (spec L2).
///
/// Three kinds of link name a note and only two of them belong here. A link
/// that resolved to `path` is a backlink. A link that names `path` and one
/// other note picks neither, so it is a backlink of both, marked
/// [`BacklinkCertainty::Ambiguous`]. A link that resolved to a *different*
/// note of the same name, and a link that resolved to nothing at all, are
/// backlinks of no note: the first belongs to the note it reached and the
/// second to none.
///
/// The snippet is cut from the text `files_fts` holds, so no note is read off
/// disk to answer this and a placeholder is not materialised by a list being
/// opened. A note indexed by name alone therefore links to nothing: indexing
/// it drops the facts derived from its text, the links among them, and it
/// leaves every list until something downloads it.
pub(crate) fn backlinks(
    conn: &Connection,
    names: &NameIndex,
    path: &str,
) -> StorageResult<Vec<BacklinkRow>> {
    let (resolved, unresolved) = backlink_rows(conn, path)?;
    let ambiguous = ambiguous_for(names, path, unresolved);
    describe_backlinks(conn, resolved, ambiguous)
}

/// The links that resolved to `path`, through the index on `links(to_path)`,
/// and the links that name it and reached no one note, through the index on
/// `links(name_key)` (migration 044). Neither read scans the folder's links
/// (RC-34).
pub(crate) fn backlink_rows(
    conn: &Connection,
    path: &str,
) -> StorageResult<(Vec<LinkRow>, Vec<LinkRow>)> {
    let resolved = links_to(conn, path)?;
    let keys = links::candidate_name_keys(path);
    let mut unresolved = Vec::new();
    for (at, key) in keys.iter().enumerate() {
        if keys[..at].contains(key) {
            continue;
        }
        unresolved.extend(link_rows(
            conn,
            "SELECT from_path, to_target, to_path, kind, line, col FROM links
              WHERE name_key = ?1 AND to_path IS NULL ORDER BY from_path, line, col",
            key,
        )?);
    }
    Ok((resolved, unresolved))
}

/// The links among `unresolved` that name `path` and at least one other note,
/// each with the other notes it might mean.
///
/// Stored targets are read the way every resolution pass reads them, so an
/// ambiguous link is listed under the same notes the editor offers to open.
pub(crate) fn ambiguous_for(
    names: &NameIndex,
    path: &str,
    unresolved: Vec<LinkRow>,
) -> Vec<(LinkRow, Vec<String>)> {
    let mut found = Vec::new();
    for row in unresolved {
        let target = links::stored_target(&row.to_target);
        if let Resolution::Ambiguous(candidates) = names.resolve(&target, &row.from_path) {
            if candidates.iter().any(|candidate| candidate == path) {
                // The list the row is shown under is the one note it is
                // already filed against, so what it carries is the others.
                let others = candidates
                    .into_iter()
                    .filter(|candidate| candidate != path)
                    .collect();
                found.push((row, others));
            }
        }
    }
    found
}

/// Orders the backlinks of one note and attaches the linking note's name, the
/// link's alias and its sentence to each, reading each linking note's text
/// once however many links it holds.
pub(crate) fn describe_backlinks(
    conn: &Connection,
    resolved: Vec<LinkRow>,
    ambiguous: Vec<(LinkRow, Vec<String>)>,
) -> StorageResult<Vec<BacklinkRow>> {
    let mut found: Vec<(LinkRow, BacklinkCertainty, Vec<String>)> = resolved
        .into_iter()
        .map(|row| (row, BacklinkCertainty::Resolved, Vec::new()))
        .chain(
            ambiguous
                .into_iter()
                .map(|(row, others)| (row, BacklinkCertainty::Ambiguous, others)),
        )
        .collect();
    found.sort_by(|(left, ..), (right, ..)| {
        (&left.from_path, left.line, left.col).cmp(&(&right.from_path, right.line, right.col))
    });

    let mut described = Vec::with_capacity(found.len());
    let mut start = 0usize;
    while start < found.len() {
        let from_path = found[start].0.from_path.clone();
        let end = found[start..]
            .iter()
            .position(|(row, ..)| row.from_path != from_path)
            .map_or(found.len(), |offset| start + offset);

        let content = note_text(conn, &from_path)?;
        // Taken from the path, not from the indexed name: the path is the
        // note's identity and is spelled the same on every row.
        let from_name = notes::note_display_name(&from_path);
        // The same parser that wrote the rows, so the alias and the row can
        // never disagree about which link is which.
        let scanned = links::scan(&content);
        for (row, certainty, candidates) in &found[start..end] {
            let written = scanned
                .iter()
                .find(|link| link.line == row.line && link.col == row.col);
            described.push(BacklinkRow {
                from_path: from_path.clone(),
                from_name: from_name.clone(),
                to_target: row.to_target.clone(),
                alias: written.and_then(|link| link.alias.clone()),
                kind: row.kind.clone(),
                line: row.line,
                col: row.col,
                context: written
                    .map(|link| snippet::sentence_at(&content, link.byte_range.start))
                    .unwrap_or_default(),
                certainty: *certainty,
                candidates: candidates.clone(),
            });
        }
        start = end;
    }
    Ok(described)
}

/// The indexed text of the note at `path`: empty when the index holds the note
/// by name alone, and empty when it does not hold it at all.
fn note_text(conn: &Connection, path: &str) -> StorageResult<String> {
    let text: Option<String> = conn
        .prepare_cached(
            "SELECT x.content FROM files_fts x
             JOIN files f ON f.rowid = x.rowid
             WHERE f.path = ?1",
        )?
        .query_row(params![path], |row| row.get(0))
        .optional()?;
    Ok(text.unwrap_or_default())
}

/// Everything the index holds about the note at `path`.
pub(crate) fn facts(conn: &Connection, path: &str) -> StorageResult<NoteFactsRow> {
    Ok(NoteFactsRow {
        links: links_from(conn, path)?,
        properties: property_rows(conn, path)?,
        tags: tag_rows(conn, path)?,
        headings: heading_rows(conn, path)?,
    })
}

/// The note's frontmatter properties, each value as the JSON it is stored as,
/// in the order they were written.
fn property_rows(conn: &Connection, path: &str) -> StorageResult<Vec<(String, String)>> {
    let mut stmt = conn
        .prepare_cached("SELECT key, value_json FROM properties WHERE path = ?1 ORDER BY rowid")?;
    let rows = stmt
        .query_map(params![path], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The note's tags with the line each is on.
fn tag_rows(conn: &Connection, path: &str) -> StorageResult<Vec<(String, u32)>> {
    let mut stmt =
        conn.prepare_cached("SELECT tag, line FROM tags WHERE path = ?1 ORDER BY line, rowid")?;
    let rows = stmt
        .query_map(params![path], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as u32))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// The note's headings, in document order.
fn heading_rows(conn: &Connection, path: &str) -> StorageResult<Vec<HeadingRow>> {
    let mut stmt = conn.prepare_cached(
        "SELECT level, text, line, slug FROM headings WHERE path = ?1 ORDER BY line, rowid",
    )?;
    let rows = stmt
        .query_map(params![path], |row| {
            Ok(HeadingRow {
                level: row.get::<_, i64>(0)? as u8,
                text: row.get(1)?,
                line: row.get::<_, i64>(2)? as u32,
                slug: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// How much of the note at `path` the index holds, or `None` when it holds no
/// row for it at all.
pub(crate) fn indexed_by(conn: &Connection, path: &str) -> StorageResult<Option<IndexedBy>> {
    let value: Option<String> = conn
        .prepare_cached("SELECT indexed_by FROM files WHERE path = ?1")?
        .query_row(params![path], |row| row.get(0))
        .optional()?;
    Ok(value.map(|value| IndexedBy::from_stored(&value)))
}

/// Every tag the index holds, with the number of notes carrying each, most
/// carried first.
pub(crate) fn all_tags(conn: &Connection) -> StorageResult<Vec<(String, usize)>> {
    let mut stmt = conn.prepare_cached(
        "SELECT tag, COUNT(DISTINCT path) AS notes FROM tags
         GROUP BY tag ORDER BY notes DESC, tag ASC",
    )?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)? as usize))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every note carrying `tag` or a tag under it, in path order.
pub(crate) fn paths_for_tag(conn: &Connection, tag: &str) -> StorageResult<Vec<String>> {
    let mut stmt = conn.prepare_cached(
        "SELECT DISTINCT tags.path FROM tags
           JOIN files ON files.path = tags.path
          WHERE tags.tag = ?1
             OR substr(tags.tag, 1, length(?1) + 1) = ?1 || '/'
          ORDER BY tags.path",
    )?;
    let rows = stmt
        .query_map(params![tag.to_lowercase()], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

/// Every note in the folder and every resolved link among them.
pub(crate) fn graph(conn: &Connection, notes_root: &Path) -> StorageResult<GraphRows> {
    let prefix = root_prefix(notes_root);
    let mut stmt = conn.prepare_cached("SELECT path FROM files ORDER BY path")?;
    let nodes = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|path| is_note_file(path))
        .map(|path| GraphNode {
            name: notes::note_display_name(&path),
            folder: folder_segment(&path, &prefix),
            path,
        })
        .collect::<Vec<_>>();

    let known: HashSet<&str> = nodes.iter().map(|node| node.path.as_str()).collect();
    let mut stmt = conn.prepare_cached(
        "SELECT from_path, to_path, COUNT(*) FROM links
          WHERE to_path IS NOT NULL AND to_path <> from_path
          GROUP BY from_path, to_path
          ORDER BY from_path, to_path",
    )?;
    let edges = stmt
        .query_map([], |row| {
            Ok(GraphEdge {
                from_path: row.get(0)?,
                to_path: row.get(1)?,
                count: row.get::<_, i64>(2)? as usize,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .filter(|edge| {
            known.contains(edge.from_path.as_str()) && known.contains(edge.to_path.as_str())
        })
        .collect();

    Ok(GraphRows { nodes, edges })
}

/// The line the heading `slug` is on in the note at `path`, or `None` when the
/// note has no such heading.
pub(crate) fn heading_line(
    conn: &Connection,
    path: &str,
    slug: &str,
) -> StorageResult<Option<u32>> {
    let line: Option<i64> = conn
        .prepare_cached("SELECT line FROM headings WHERE path = ?1 AND slug = ?2 LIMIT 1")?
        .query_row(params![path, slug], |row| row.get(0))
        .optional()?;
    Ok(line.map(|line| line as u32))
}

impl NotesIndex<'_> {
    /// Up to `limit` ranked content hits, each carrying the note's path, name,
    /// matching line and a highlighted snippet. See [`search_hits`].
    pub fn search_hits(
        &self,
        query: &str,
        terms: &[String],
        limit: usize,
    ) -> StorageResult<Vec<SearchHit>> {
        search_hits(self.conn, query, terms, limit)
    }

    /// Total number of notes matching `query`, independent of any result
    /// limit, so the UI can report "N of M" honestly.
    pub fn count(&self, query: &str) -> StorageResult<usize> {
        count(self.conn, query)
    }

    /// Up to `limit` ranked name-only hits under `notes_root`, for quick open.
    /// See [`search_names`].
    pub fn search_names(
        &self,
        query: &str,
        notes_root: &Path,
        limit: usize,
    ) -> StorageResult<Vec<FileHit>> {
        Ok(search_names(
            &NameIndex::load(self.conn)?,
            query,
            notes_root,
            limit,
        ))
    }

    /// Every indexed path with the size and mtime it was indexed at, for
    /// reconciliation.
    pub fn snapshot(&self) -> StorageResult<Vec<(String, u64, i64)>> {
        schema::snapshot(self.conn)
    }

    /// Every link written in the note at `path`.
    pub fn links_from(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        links_from(self.conn, path)
    }

    /// Every link that resolved to the note at `path`.
    ///
    /// An unresolved link and an ambiguous one are absent by construction:
    /// their `to_path` is `NULL`, which is the honest record of a link that
    /// points at no one note. [`NotesIndex::backlinks`] adds the ambiguous
    /// ones back, flagged, because a link that might mean this note is a fact
    /// about this note.
    pub fn links_to(&self, path: &str) -> StorageResult<Vec<LinkRow>> {
        links_to(self.conn, path)
    }

    /// The notes that link to the note at `path`, each link with the sentence
    /// it sits in. See [`backlinks`].
    pub fn backlinks(&self, path: &str) -> StorageResult<Vec<BacklinkRow>> {
        backlinks(self.conn, &NameIndex::load(self.conn)?, path)
    }

    /// Everything the index holds about the note at `path`.
    pub fn facts(&self, path: &str) -> StorageResult<NoteFactsRow> {
        facts(self.conn, path)
    }

    /// How much of the note at `path` the index holds, or `None` when it holds
    /// no row for it at all.
    ///
    /// The three answers a caller needs are `None` (not indexed),
    /// `Some(IndexedBy::Name)` (the row carries the file's name and nothing
    /// else, so its links, properties and tags are empty because nothing was
    /// read) and `Some(IndexedBy::Content)` (empty means empty). Without this
    /// the first two are indistinguishable from the third.
    pub fn indexed_by(&self, path: &str) -> StorageResult<Option<IndexedBy>> {
        indexed_by(self.conn, path)
    }

    /// Every tag the index holds, with the number of notes carrying each.
    ///
    /// Tags come back as the `tags` table stores them, without the leading
    /// `#`, which is also what [`NoteFactsRow::tags`] carries. Ordered by note
    /// count descending, then by tag, so the folder's common tags come first
    /// and the order is stable between two calls over the same rows. A note
    /// tagged twice with the same tag counts once.
    pub fn all_tags(&self) -> StorageResult<Vec<(String, usize)>> {
        all_tags(self.conn)
    }

    /// Every note carrying `tag` or a tag under it, in path order.
    ///
    /// `project` answers with the notes carrying `#project`, `#project/alpha`
    /// and `#project/alpha/x`; `project/alpha` answers with its own subtree.
    /// That is what lets the tag list show each tag once under its parent and
    /// a parent row filter its whole family. A sibling that merely shares a
    /// prefix (`projects`, `project-x`) is outside the family, which is why
    /// the prefix test is a `substr` against `tag || '/'` and not a `LIKE`: a
    /// `_` inside a tag is a character, never a wildcard. A note tagging
    /// itself twice comes back once.
    ///
    /// Case is folded on the way in, because it was folded on the way into the
    /// rows: `Project` and `project` are one tag, and asking with the casing a
    /// note was written in finds it (ADR-036).
    ///
    /// One statement, joined to `files` so a tag row left behind by a file the
    /// index no longer holds cannot name a note that is gone.
    pub fn paths_for_tag(&self, tag: &str) -> StorageResult<Vec<String>> {
        paths_for_tag(self.conn, tag)
    }

    /// Every note in the folder and every resolved link among them.
    ///
    /// One statement per table, never a query per note: a folder of five
    /// thousand notes is one read of `files` and one grouped read of `links`,
    /// which is what keeps a graph the user opens from costing what a walk
    /// costs.
    ///
    /// What is left out is as much of the answer as what is in it.
    ///
    /// - A link with no `to_path` is a link that reached no one note: it is
    ///   unresolved, or it is ambiguous and the resolver refused to pick
    ///   (ADR-034). Drawing it would draw a guess, so it is not an edge. The
    ///   note it was written in is still a node.
    /// - A note linking to itself is dropped: it is a loop on one node and it
    ///   says nothing about the folder's shape.
    /// - Links written more than once between the same pair collapse into
    ///   `count`, so a note referenced twelve times is one edge with a weight
    ///   rather than twelve lines drawn over each other.
    /// - Only Markdown files are nodes. The index also holds `.txt` and
    ///   `.text`, which are findable and openable but are plain text (ADR-041
    ///   §2), so an edge with either end outside the node set is dropped
    ///   rather than drawn to a node that is not there.
    pub fn graph(&self, notes_root: &Path) -> StorageResult<GraphRows> {
        graph(self.conn, notes_root)
    }

    /// Every indexed note a link naming `name` could mean, in byte order.
    ///
    /// The list [`Resolution::Ambiguous`] hands the user, and the input the
    /// resolver ranks. Nothing here picks one.
    pub fn candidate_paths(&self, name: &str) -> StorageResult<Vec<String>> {
        Ok(NameIndex::load(self.conn)?.candidate_paths(name))
    }

    /// Resolves `target` — the inside of a `[[…]]`, alias and heading
    /// included — as seen from the note at `from_path`.
    pub fn resolve_link(&self, from_path: &str, target: &str) -> StorageResult<Resolution> {
        let parsed = links::parse_wikilink(target);
        Ok(NameIndex::load(self.conn)?.resolve(&parsed, from_path))
    }

    /// The line the heading `slug` is on in the note at `path`, or `None` when
    /// the note has no such heading.
    ///
    /// A heading a link names but the note does not have leaves the link
    /// resolved and the heading unreported: the note still opens.
    pub fn heading_line(&self, path: &str, slug: &str) -> StorageResult<Option<u32>> {
        heading_line(self.conn, path, slug)
    }
}
