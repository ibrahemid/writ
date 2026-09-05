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

    /// The target's path as the preview shows it: relative to the notes
    /// folder, with forward slashes.
    ///
    /// A click posts the href to the app, which shows it before it opens
    /// anything, so what it reads as matters. A path outside the notes folder
    /// is shown whole rather than trimmed to something it is not.
    fn href_for(&self, path: &str) -> String {
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
        assert_eq!(rendered.href.as_deref(), Some("Note.md"));
        assert_eq!(rendered.label, "Note");
    }

    #[test]
    fn a_note_in_a_folder_keeps_its_folder_in_the_href() {
        let (_dir, resolver) = fixture();
        let rendered = resolver.resolve("Deep");
        assert_eq!(rendered.href.as_deref(), Some("folder/Deep.md"));
    }

    #[test]
    fn a_note_that_is_not_there_is_not_a_link() {
        let (_dir, resolver) = fixture();
        let rendered = resolver.resolve("Nowhere");
        assert!(!rendered.resolved);
        assert!(rendered.href.is_none());
        assert_eq!(rendered.label, "Nowhere");
    }

    #[test]
    fn an_alias_is_the_label_and_a_heading_is_not() {
        let (_dir, resolver) = fixture();
        assert_eq!(resolver.resolve("Note|the note").label, "the note");
        assert_eq!(resolver.resolve("Note#Section").label, "Note#Section");
        assert!(resolver.resolve("Note#Section").resolved);
    }
}
