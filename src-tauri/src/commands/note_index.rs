//! Reading the notes index: resolving a link, and what a note says about
//! itself.
//!
//! The rules are `writ_core::notes::links` and the rows are
//! `writ_storage::notes_index` (ADR-034). This module is the adapter between
//! them and the editor: it spells incoming paths through
//! [`writ_storage::notes_index::index_key`], so a path that reaches Rust from a
//! tab keys the same rows the walk wrote, and it hands the editor an
//! `Ambiguous` result whole rather than picking one of the candidates.

use std::path::Path;

use serde::Serialize;
use tauri::State;
use writ_core::notes::links::{self, Resolution};
use writ_storage::notes_index::{self, NotesIndexStore};

use crate::state::AppState;

/// Caps the `[[` completion list. Same ceiling quick open uses: a list the user
/// scrolls is a list they should have typed more into.
const NAME_CANDIDATE_LIMIT: usize = 50;

/// What a link target resolved to.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LinkResolutionDto {
    /// `resolved`, `ambiguous` or `missing`.
    pub status: String,
    /// The note the target resolved to, present only for `resolved`.
    pub path: Option<String>,
    /// The notes the target could mean, present only for `ambiguous`.
    pub candidates: Vec<String>,
    /// The line of the heading the target named, when it named one the target
    /// note has.
    pub heading_line: Option<u32>,
}

/// One link the index holds.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct LinkDto {
    pub to_target: String,
    pub to_path: Option<String>,
    pub kind: String,
    pub line: u32,
    pub col: u32,
}

/// One note that links to the note being looked at.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct BacklinkDto {
    pub from_path: String,
    pub from_name: String,
    pub to_target: String,
    pub alias: Option<String>,
    pub kind: String,
    pub line: u32,
    pub col: u32,
    /// The sentence the link sits in, empty for a note the index holds by name
    /// alone.
    pub context: String,
    /// `resolved` or `ambiguous`.
    pub certainty: String,
    /// The other notes an ambiguous link might mean, by path. Empty when the
    /// link means this note and no other.
    pub candidates: Vec<String>,
}

/// One frontmatter property, its value as the JSON it is stored as.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct PropertyDto {
    pub key: String,
    pub value_json: String,
}

/// One tag and the line it is on.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TagDto {
    pub tag: String,
    pub line: u32,
}

/// One heading and the anchor a link matches it by.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct HeadingDto {
    pub level: u8,
    pub text: String,
    pub line: u32,
    pub slug: String,
}

/// Everything the index holds about one note beyond its file row.
#[derive(Debug, Serialize, PartialEq, Eq, Default)]
pub struct NoteFactsDto {
    pub links: Vec<LinkDto>,
    pub properties: Vec<PropertyDto>,
    pub tags: Vec<TagDto>,
    pub headings: Vec<HeadingDto>,
}

/// One tag the folder carries, with the number of notes carrying it.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct TagCountDto {
    /// The tag without its leading `#`, as the index stores it.
    pub tag: String,
    /// How many notes carry it. A note tagged twice counts once.
    pub count: usize,
}

/// One note in the folder's link graph.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GraphNodeDto {
    pub path: String,
    pub name: String,
    /// The first folder under the notes root, empty for a note in the root.
    pub folder: String,
}

/// A link between two notes, and how many times it is written.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct GraphEdgeDto {
    pub from_path: String,
    pub to_path: String,
    pub count: usize,
}

/// The whole folder: every note, and every resolved link among them.
#[derive(Debug, Serialize, PartialEq, Eq, Default)]
pub struct NoteGraphDto {
    pub nodes: Vec<GraphNodeDto>,
    pub edges: Vec<GraphEdgeDto>,
}

/// One note offered to a `[[` completion.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct NoteNameHit {
    pub path: String,
    pub name: String,
}

/// Resolves the inside of a `[[…]]` written in the note at `from_path`.
///
/// An ambiguous target is reported as ambiguous with its candidates, never as
/// a resolution to one of them: the editor shows the choice and the rename in
/// U11 refuses to rewrite it (ADR-034).
pub fn resolve_note_link_inner(
    index: &NotesIndexStore,
    from_path: &str,
    target: &str,
) -> Result<LinkResolutionDto, String> {
    let from = notes_index::index_key(Path::new(from_path));
    let resolution = index
        .resolve_link(&from, target)
        .map_err(|e| e.to_string())?;
    let heading = links::parse_wikilink(target).heading;

    Ok(match resolution {
        Resolution::Resolved(path) => {
            let heading_line = match &heading {
                Some(text) => index
                    .heading_line(&path, &links::heading_slug(text))
                    .map_err(|e| e.to_string())?,
                None => None,
            };
            LinkResolutionDto {
                status: "resolved".to_string(),
                path: Some(path),
                candidates: Vec::new(),
                heading_line,
            }
        }
        Resolution::Ambiguous(candidates) => LinkResolutionDto {
            status: "ambiguous".to_string(),
            path: None,
            candidates,
            heading_line: None,
        },
        Resolution::Missing => LinkResolutionDto {
            status: "missing".to_string(),
            path: None,
            candidates: Vec::new(),
            heading_line: None,
        },
    })
}

/// Everything the index holds about the note at `path`.
pub fn note_facts_inner(index: &NotesIndexStore, path: &str) -> Result<NoteFactsDto, String> {
    let key = notes_index::index_key(Path::new(path));
    let facts = index.facts(&key).map_err(|e| e.to_string())?;
    Ok(NoteFactsDto {
        links: facts
            .links
            .into_iter()
            .map(|link| LinkDto {
                to_target: link.to_target,
                to_path: link.to_path,
                kind: link.kind,
                line: link.line,
                col: link.col,
            })
            .collect(),
        properties: facts
            .properties
            .into_iter()
            .map(|(key, value_json)| PropertyDto { key, value_json })
            .collect(),
        tags: facts
            .tags
            .into_iter()
            .map(|(tag, line)| TagDto { tag, line })
            .collect(),
        headings: facts
            .headings
            .into_iter()
            .map(|heading| HeadingDto {
                level: heading.level,
                text: heading.text,
                line: heading.line,
                slug: heading.slug,
            })
            .collect(),
    })
}

/// Every tag the folder carries, most-used first.
///
/// A folder with no tags answers with an empty list: the sidebar renders
/// nothing rather than a row saying there is nothing.
pub fn note_all_tags_inner(index: &NotesIndexStore) -> Result<Vec<TagCountDto>, String> {
    Ok(index
        .all_tags()
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(tag, count)| TagCountDto { tag, count })
        .collect())
}

/// The whole folder as notes and the links among them.
///
/// Only resolved links are edges. A target naming two notes picks neither
/// (ADR-034), so it reaches the graph as no edge at all rather than as a line
/// drawn to a guess.
pub fn note_graph_inner(
    index: &NotesIndexStore,
    notes_root: &Path,
) -> Result<NoteGraphDto, String> {
    let rows = index.graph(notes_root).map_err(|e| e.to_string())?;
    Ok(NoteGraphDto {
        nodes: rows
            .nodes
            .into_iter()
            .map(|node| GraphNodeDto {
                path: node.path,
                name: node.name,
                folder: node.folder,
            })
            .collect(),
        edges: rows
            .edges
            .into_iter()
            .map(|edge| GraphEdgeDto {
                from_path: edge.from_path,
                to_path: edge.to_path,
                count: edge.count,
            })
            .collect(),
    })
}

/// The notes that link to the note at `path`.
///
/// A note nothing links to answers with an empty list, which is a list with
/// nothing in it and not a row saying so: zero backlinks is nothing rendered
/// (spec L2).
pub fn note_backlinks_inner(
    index: &NotesIndexStore,
    path: &str,
) -> Result<Vec<BacklinkDto>, String> {
    let key = notes_index::index_key(Path::new(path));
    Ok(index
        .backlinks(&key)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|row| BacklinkDto {
            from_path: row.from_path,
            from_name: row.from_name,
            to_target: row.to_target,
            alias: row.alias,
            kind: row.kind,
            line: row.line,
            col: row.col,
            context: row.context,
            certainty: row.certainty.as_str().to_string(),
            candidates: row.candidates,
        })
        .collect())
}

/// Ranked note names for a `[[` completion.
///
/// Served by the same name index quick open reads, so completion and the
/// palette agree on what exists and neither one walks the folder again
/// (spec L1).
pub fn note_name_candidates_inner(
    index: &NotesIndexStore,
    query: &str,
    notes_root: &Path,
    limit: Option<usize>,
) -> Result<Vec<NoteNameHit>, String> {
    if query.trim().is_empty() {
        return Ok(Vec::new());
    }
    let limit = limit
        .unwrap_or(NAME_CANDIDATE_LIMIT)
        .min(NAME_CANDIDATE_LIMIT);
    Ok(index
        .search_names(query, notes_root, limit)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|hit| NoteNameHit {
            path: hit.path,
            name: hit.name,
        })
        .collect())
}

/// The line the heading `slug` names inside the note at `path`, or `None` when
/// the note has no such heading.
///
/// The preview writes a resolved `[[Note#Section]]` as an href whose fragment
/// is the heading's anchor, so the click has an anchor where the editor has
/// the heading text it was written with. `heading_slug` is applied to whatever
/// arrives: it leaves an anchor alone and turns a heading text into one, which
/// is what lets both surfaces land on the same line (ADR-034).
pub fn note_heading_line_inner(
    index: &NotesIndexStore,
    path: &str,
    slug: &str,
) -> Result<Option<u32>, String> {
    let key = notes_index::index_key(Path::new(path));
    index
        .heading_line(&key, &links::heading_slug(slug))
        .map_err(|e| e.to_string())
}

/// Resolves a link target written in one note. See [`resolve_note_link_inner`].
#[tauri::command]
pub fn resolve_note_link(
    state: State<'_, AppState>,
    from_path: String,
    target: String,
) -> Result<LinkResolutionDto, String> {
    resolve_note_link_inner(&state.notes_index, &from_path, &target)
}

/// The links, properties, tags and headings of one note. See
/// [`note_facts_inner`].
#[tauri::command]
pub fn note_facts(state: State<'_, AppState>, path: String) -> Result<NoteFactsDto, String> {
    note_facts_inner(&state.notes_index, &path)
}

/// Every tag the folder carries. See [`note_all_tags_inner`].
#[tauri::command]
pub fn note_all_tags(state: State<'_, AppState>) -> Result<Vec<TagCountDto>, String> {
    note_all_tags_inner(&state.notes_index)
}

/// The folder's notes and the links among them. See [`note_graph_inner`].
#[tauri::command]
pub fn note_graph(state: State<'_, AppState>) -> Result<NoteGraphDto, String> {
    note_graph_inner(&state.notes_index, &state.notes_root())
}

/// The notes that link to one note. See [`note_backlinks_inner`].
#[tauri::command]
pub fn note_backlinks(
    state: State<'_, AppState>,
    path: String,
) -> Result<Vec<BacklinkDto>, String> {
    note_backlinks_inner(&state.notes_index, &path)
}

/// The line a heading sits on. See [`note_heading_line_inner`].
#[tauri::command]
pub fn note_heading_line(
    state: State<'_, AppState>,
    path: String,
    slug: String,
) -> Result<Option<u32>, String> {
    note_heading_line_inner(&state.notes_index, &path, &slug)
}

/// Note names for a `[[` completion. See [`note_name_candidates_inner`].
#[tauri::command]
pub fn note_name_candidates(
    state: State<'_, AppState>,
    query: String,
    limit: Option<usize>,
) -> Result<Vec<NoteNameHit>, String> {
    note_name_candidates_inner(&state.notes_index, &query, &state.notes_root(), limit)
}
