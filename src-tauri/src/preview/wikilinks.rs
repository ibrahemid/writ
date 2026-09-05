//! Resolving a preview's `[[…]]` against the notes index.
//!
//! The rules are `writ_core::notes::links` and the rows are
//! `writ_storage::notes_index`, the same pair the editor reads through
//! `resolve_note_link` (ADR-034). This module is the adapter that lets
//! `writ-render` ask them a question without depending on either: the crate
//! sees only the [`WikilinkResolver`] trait, so it keeps building to wasm for
//! the site, where no resolver is supplied and a wikilink stays text.

use std::path::Path;
use std::sync::Arc;

use writ_core::notes::links::{self, Resolution};
use writ_render::{WikilinkRender, WikilinkResolver};

/// What a preview link to a note is written with.
///
/// The frontend recognises it and opens the note; anything else posted from
/// the frame goes to the external-link policy the way it always has. Kept in
/// step with `NOTE_LINK_SCHEME` in `src/lib/wikilink.ts`.
pub const NOTE_LINK_SCHEME: &str = "writ-note:";

/// `path` with the characters that would be read as something other than path
/// text escaped, so the frontend decodes back to the name on disk.
fn encode_href(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for ch in path.chars() {
        match ch {
            '%' => out.push_str("%25"),
            '#' => out.push_str("%23"),
            '?' => out.push_str("%3F"),
            _ => out.push(ch),
        }
    }
    out
}
use writ_storage::notes_index::{self, NotesIndexStore};

/// Resolves the wikilinks of one rendered note.
pub struct IndexWikilinks {
    index: Arc<NotesIndexStore>,
    /// The notes folder, canonical. Every href is relative to it.
    notes_root: String,
    /// Where the note being rendered sits, canonical.
    ///
    /// The index keys a note by its own canonical path, and this is the
    /// canonical path of its folder. `resolve` reads it only to rank two
    /// notes of the same name by how near they are to the linking note, and
    /// the folder ranks identically to any file in it, so the note's own file
    /// name is not needed here.
    from: String,
}

impl IndexWikilinks {
    /// The resolver for a note in `note_dir` inside `notes_root`.
    pub fn new(index: Arc<NotesIndexStore>, notes_root: &Path, note_dir: &Path) -> Self {
        Self {
            index,
            notes_root: notes_index::index_key(notes_root),
            from: notes_index::index_key(note_dir),
        }
    }

    /// The target's destination as the preview writes it: the
    /// [`NOTE_LINK_SCHEME`] prefix and the path relative to the notes folder,
    /// with forward slashes.
    ///
    /// A click posts the href to the app, and a note is not a web address: the
    /// scheme is what tells the app to open the note rather than hand the
    /// string to the external-link policy, which would refuse a relative path
    /// as not being an address at all. The app joins it back onto the notes
    /// folder and refuses anything that lands outside, so the scheme grants
    /// nothing a note in that folder does not already have.
    fn href_for(&self, path: &str) -> String {
        format!(
            "{NOTE_LINK_SCHEME}{}",
            encode_href(&self.relative_to_root(path))
        )
    }

    /// The path the preview shows, relative to the notes folder. A path
    /// outside it is kept whole rather than trimmed to something it is not.
    fn relative_to_root(&self, path: &str) -> String {
        let separator = if self.notes_root.contains('\\') {
            '\\'
        } else {
            '/'
        };
        let prefix = format!("{}{separator}", self.notes_root.trim_end_matches(separator));
        let relative = path.strip_prefix(&prefix).unwrap_or(path);
        relative.replace('\\', "/")
    }
}

impl WikilinkResolver for IndexWikilinks {
    fn resolve(&self, inner: &str) -> WikilinkRender {
        let parsed = links::parse_wikilink(inner);
        // The alias is what the author wanted read; without one the target is
        // shown as it was written, heading and all.
        let label = parsed
            .alias
            .clone()
            .unwrap_or_else(|| inner.split('|').next().unwrap_or(inner).trim().to_string());
        let resolution = self.index.resolve_link(&self.from, inner);
        match resolution {
            Ok(Resolution::Resolved(path)) => WikilinkRender {
                href: Some(self.href_for(&path)),
                label,
                resolved: true,
            },
            // A target that names several notes picks none of them, and one
            // that names no note has nothing to point at. Both read as text.
            Ok(Resolution::Ambiguous(_)) | Ok(Resolution::Missing) => WikilinkRender {
                href: None,
                label,
                resolved: false,
            },
            Err(error) => {
                tracing::debug!(error = %error, "preview wikilink not resolved");
                WikilinkRender {
                    href: None,
                    label,
                    resolved: false,
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A notes folder with `Note.md` in it and a second note in `folder/`,
    /// indexed, plus the resolver for a note sitting at the root.
    fn fixture() -> (tempfile::TempDir, IndexWikilinks) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("notes");
        fs::create_dir_all(root.join("folder")).expect("folders");
        fs::write(root.join("Note.md"), "# Note\n").expect("note");
        fs::write(root.join("folder/Deep.md"), "# Deep\n").expect("deep");
        fs::write(root.join("From.md"), "[[Note]]\n").expect("from");

        let db_path = dir.path().join("writ.db");
        let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);

        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        index
            .reconcile(&root, &|| false, &|_| false)
            .expect("reconcile");
        let resolver = IndexWikilinks::new(index, &root, &root);
        (dir, resolver)
    }

    #[test]
    fn a_note_that_exists_becomes_a_link_relative_to_the_notes_folder() {
        let (_dir, resolver) = fixture();
        let rendered = resolver.resolve("Note");
        assert!(rendered.resolved);
        assert_eq!(rendered.href.as_deref(), Some("writ-note:Note.md"));
        assert_eq!(rendered.label, "Note");
    }

    #[test]
    fn a_note_in_a_folder_keeps_its_folder_in_the_href() {
        let (_dir, resolver) = fixture();
        let rendered = resolver.resolve("Deep");
        assert_eq!(rendered.href.as_deref(), Some("writ-note:folder/Deep.md"));
    }

    #[test]
    fn a_note_that_is_not_there_is_not_a_link() {
        let (_dir, resolver) = fixture();
        let rendered = resolver.resolve("Nowhere");
        assert!(!rendered.resolved);
        assert!(rendered.href.is_none());
        assert_eq!(rendered.label, "Nowhere");
    }

    // A note whose name carries one of these is still one path segment, and
    // the frontend decodes it back before it joins it onto the notes folder.
    #[test]
    fn a_name_that_would_read_as_something_else_is_escaped() {
        assert_eq!(encode_href("a#b.md"), "a%23b.md");
        assert_eq!(encode_href("a?b.md"), "a%3Fb.md");
        assert_eq!(encode_href("100%.md"), "100%25.md");
        assert_eq!(encode_href("folder/Note.md"), "folder/Note.md");
        assert_eq!(encode_href("Café.md"), "Café.md");
    }

    #[test]
    fn an_alias_is_the_label_and_a_heading_is_not() {
        let (_dir, resolver) = fixture();
        assert_eq!(resolver.resolve("Note|the note").label, "the note");
        assert_eq!(resolver.resolve("Note#Section").label, "Note#Section");
        assert!(resolver.resolve("Note#Section").resolved);
    }
}
