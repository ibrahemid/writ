//! Resolving a preview's `![[…]]` against the notes index.
//!
//! The sibling of [`super::wikilinks`], asking the same index the same
//! question and reading it under the same rules (ADR-034), with one thing
//! more: an embed shows the target's text, so this reads the file. What it
//! reads and how deep it goes are `writ-render`'s policy — this module answers
//! and hands over bytes.

use std::path::Path;
use std::sync::Arc;

use writ_core::notes::guard::is_not_downloaded;
use writ_core::notes::links::Resolution;
use writ_core::preview::AssetScope;
use writ_render::{EmbedBase, EmbedResolution, EmbedTarget, NoteEmbedResolver, MAX_EMBED_DEPTH};
use writ_storage::notes_index::{self, NotesIndexStore};

use super::renderers::markdown::asset_url;
use super::wikilinks::IndexWikilinks;
use writ_render::WikilinkResolver;

/// Largest note read for an embed.
///
/// A preview that embeds a note pulls its whole text into the rendered
/// document, and the document is rebuilt on every keystroke the editor
/// debounces. The ceiling is well under the renderer's own 50 MB refusal
/// because the cost here is paid per embed, not per document.
const MAX_EMBED_BYTES: u64 = 1024 * 1024;

/// Reports the flags the filesystem carries for a file, or `None` where there
/// are none.
///
/// Injected rather than called so an evicted note is testable on any platform
/// and without a sync provider. Production passes
/// [`writ_storage::buffer_store::dataless_flags`].
pub type DatalessProbe = fn(&Path) -> Option<u32>;

/// Resolves the note embeds of one rendered note.
pub struct IndexEmbeds {
    index: Arc<NotesIndexStore>,
    /// Where the note being rendered sits, canonical. Two notes of the same
    /// name are ranked by how near they are to it, the same as for a link.
    from: String,
    /// Writes the target's href and label, so an embed that renders as a link
    /// is written exactly as the link form of the same target would be.
    links: IndexWikilinks,
    /// The render's asset scope with this note's folder in it. Everything else
    /// is the outer render's, the token included: the token says which
    /// document owns the scope, and an embedded note is part of that document
    /// (ADR-035). Only the folder a relative reference is read from moves.
    scope: AssetScope,
    /// How the filesystem is asked whether a file's bytes are local.
    dataless: DatalessProbe,
}

impl IndexEmbeds {
    /// The resolver for the note `scope` was built for.
    pub fn new(index: Arc<NotesIndexStore>, scope: AssetScope, dataless: DatalessProbe) -> Self {
        Self {
            from: notes_index::index_key(&scope.note_dir),
            links: IndexWikilinks::new(Arc::clone(&index), &scope.notes_root, &scope.note_dir),
            index,
            scope,
            dataless,
        }
    }

    /// The same resolver rebased on the folder holding `note_path`.
    ///
    /// The target came out of the index, so its folder is already inside the
    /// notes root: this moves the base a reference is read from, never the
    /// roots it is confined to.
    fn rebased_on(&self, note_path: &str) -> Self {
        let note_dir = Path::new(note_path)
            .parent()
            .unwrap_or_else(|| Path::new(note_path))
            .to_path_buf();
        Self::new(
            Arc::clone(&self.index),
            AssetScope {
                note_dir,
                ..self.scope.clone()
            },
            self.dataless,
        )
    }

    /// The target as `writ-render` writes it when it renders a link instead of
    /// the note's content.
    fn target_for(&self, inner: &str, path: &str) -> EmbedTarget {
        let rendered = self.links.resolve(inner);
        EmbedTarget {
            key: path.to_string(),
            label: rendered.label,
            href: rendered.href,
        }
    }
}

impl NoteEmbedResolver for IndexEmbeds {
    fn resolve(&self, inner: &str, depth: u8, visited: &[&str]) -> EmbedResolution {
        let path = match self.index.resolve_link(&self.from, inner) {
            Ok(Resolution::Resolved(path)) => path,
            // A target that names several notes picks none of them, and one
            // that names no note has nothing to show.
            Ok(Resolution::Ambiguous(_)) => return EmbedResolution::Ambiguous,
            Ok(Resolution::Missing) => return EmbedResolution::Missing,
            Err(error) => {
                tracing::debug!(error = %error, "preview note embed not resolved");
                return EmbedResolution::Missing;
            }
        };
        // Nothing below this point reads the file unless its text will be
        // rendered, which is the whole reason the two limits are passed in.
        if depth >= MAX_EMBED_DEPTH || visited.contains(&path.as_str()) {
            return EmbedResolution::Cut {
                target: self.target_for(inner, &path),
            };
        }
        let file = Path::new(&path);
        // Asked of the metadata, so an embed of an evicted note never makes a
        // sync provider fetch it (ADR-028 §5).
        if is_not_downloaded((self.dataless)(file)) {
            return EmbedResolution::NotDownloaded {
                target: self.target_for(inner, &path),
            };
        }
        match std::fs::metadata(file) {
            Ok(meta) if meta.len() > MAX_EMBED_BYTES => {
                tracing::debug!(bytes = meta.len(), "preview note embed too large to render");
                return EmbedResolution::Cut {
                    target: self.target_for(inner, &path),
                };
            }
            Ok(_) => {}
            // The index has a row the disk no longer backs.
            Err(error) => {
                tracing::debug!(error = %error, "preview note embed has no file");
                return EmbedResolution::Missing;
            }
        }
        match std::fs::read_to_string(file) {
            Ok(markdown) => EmbedResolution::Resolved {
                target: self.target_for(inner, &path),
                markdown,
                base: Some(Box::new(self.rebased_on(&path))),
            },
            Err(error) => {
                tracing::debug!(error = %error, "preview note embed could not be read");
                EmbedResolution::Missing
            }
        }
    }
}

impl EmbedBase for IndexEmbeds {
    fn asset_url(&self, reference: &str) -> Option<String> {
        asset_url(&self.scope, reference)
    }

    fn wikilinks(&self) -> Option<&dyn WikilinkResolver> {
        Some(&self.links)
    }

    fn embeds(&self) -> &dyn NoteEmbedResolver {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use writ_core::notes::guard::SF_DATALESS;

    /// The probe production uses: the filesystem has nothing to report.
    fn local(_: &Path) -> Option<u32> {
        None
    }

    /// The probe for a note the sync provider has evicted.
    fn evicted(_: &Path) -> Option<u32> {
        Some(SF_DATALESS)
    }

    /// A notes folder holding `Note.md`, a second note of the same name in two
    /// folders, and the resolver for a note sitting at the root.
    fn fixture(dataless: DatalessProbe) -> (tempfile::TempDir, IndexEmbeds) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("notes");
        fs::create_dir_all(root.join("one")).expect("one");
        fs::create_dir_all(root.join("two")).expect("two");
        fs::write(root.join("Note.md"), "# Note\n\nNote body.\n").expect("note");
        fs::write(root.join("one/Twice.md"), "one\n").expect("one note");
        fs::write(root.join("two/Twice.md"), "two\n").expect("two note");
        fs::write(root.join("From.md"), "![[Note]]\n").expect("from");

        let db_path = dir.path().join("writ.db");
        let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);

        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        index
            .reconcile(&root, &|| false, &|_| false)
            .expect("reconcile");
        let resolver = IndexEmbeds::new(index, scope(&root, &root), dataless);
        (dir, resolver)
    }

    /// The asset scope of a render of a note in `note_dir`.
    fn scope(notes_root: &Path, note_dir: &Path) -> AssetScope {
        AssetScope {
            notes_root: notes_root.to_path_buf(),
            note_dir: note_dir.to_path_buf(),
            buffer_id: "b1".to_string(),
            token: "t1".to_string(),
        }
    }

    #[test]
    fn a_note_that_exists_comes_back_with_its_text() {
        let (_dir, resolver) = fixture(local);
        match resolver.resolve("Note", 0, &[]) {
            EmbedResolution::Resolved {
                target, markdown, ..
            } => {
                assert!(markdown.contains("Note body."));
                assert_eq!(target.label, "Note");
                assert_eq!(target.href.as_deref(), Some("writ-note:Note.md"));
                assert!(target.key.ends_with("Note.md"));
            }
            _ => panic!("Note names one note"),
        }
    }

    /// A folder where the same name answers in two places and the note being
    /// embedded sits in one of them: `one/Twice.md`, `two/Twice.md`,
    /// `two/Guest.md` linking `[[Twice]]` and showing `pic.png`, and
    /// `one/Host.md` embedding Guest.
    fn two_folder_fixture() -> (tempfile::TempDir, IndexEmbeds) {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("notes");
        fs::create_dir_all(root.join("one")).expect("one");
        fs::create_dir_all(root.join("two")).expect("two");
        fs::write(root.join("one/Twice.md"), "one\n").expect("one note");
        fs::write(root.join("two/Twice.md"), "two\n").expect("two note");
        fs::write(root.join("two/pic.png"), [0u8; 4]).expect("picture");
        fs::write(
            root.join("two/Guest.md"),
            "guest body [[Twice]]\n\n![](pic.png)\n",
        )
        .expect("guest");
        fs::write(root.join("one/Host.md"), "![[Guest]]\n").expect("host");

        let db_path = dir.path().join("writ.db");
        let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);

        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        index
            .reconcile(&root, &|| false, &|_| false)
            .expect("reconcile");
        let host = root.join("one");
        let resolver = IndexEmbeds::new(index, scope(&root, &host), local);
        (dir, resolver)
    }

    // The bug this pins: rendered from `one/Host.md`, Guest's own `[[Twice]]`
    // was ranked against `one/` and resolved to the wrong note, silently.
    #[test]
    fn an_embedded_note_resolves_its_links_and_images_from_its_own_folder() {
        let (_dir, resolver) = two_folder_fixture();
        let html = writ_render::render_markdown_fragment_with(
            "![[Guest]]\n",
            None,
            None,
            Some(&resolver as &dyn NoteEmbedResolver),
        )
        .html;
        assert!(html.contains("writ-note:two/Twice.md"), "{html}");
        assert!(!html.contains("writ-note:one/Twice.md"), "{html}");
        assert!(html.contains("two/pic.png"), "{html}");
    }

    #[test]
    fn a_name_two_notes_answer_to_is_ambiguous() {
        let (_dir, resolver) = fixture(local);
        assert!(matches!(
            resolver.resolve("Twice", 0, &[]),
            EmbedResolution::Ambiguous
        ));
    }

    #[test]
    fn a_name_no_note_answers_to_is_missing() {
        let (_dir, resolver) = fixture(local);
        assert!(matches!(
            resolver.resolve("Nowhere", 0, &[]),
            EmbedResolution::Missing
        ));
    }

    // The file is deleted first, so a read would fail. Coming back with the
    // placeholder rather than `Missing` is the proof no read was attempted.
    #[test]
    fn a_note_that_is_not_downloaded_is_reported_without_being_read() {
        let (dir, resolver) = fixture(evicted);
        fs::remove_file(dir.path().join("notes/Note.md")).expect("remove");
        match resolver.resolve("Note", 0, &[]) {
            EmbedResolution::NotDownloaded { target } => assert_eq!(target.label, "Note"),
            _ => panic!("an evicted note is reported, not read"),
        }
    }

    #[test]
    fn a_target_the_render_is_already_inside_is_cut_without_being_read() {
        let (dir, resolver) = fixture(local);
        let key = notes_index::index_key(&dir.path().join("notes/Note.md"));
        fs::remove_file(dir.path().join("notes/Note.md")).expect("remove");
        match resolver.resolve("Note", 0, &[key.as_str()]) {
            EmbedResolution::Cut { target } => {
                assert_eq!(target.href.as_deref(), Some("writ-note:Note.md"))
            }
            _ => panic!("a note the page is inside is cut"),
        }
    }

    #[test]
    fn a_target_at_the_depth_ceiling_is_cut_without_being_read() {
        let (dir, resolver) = fixture(local);
        fs::remove_file(dir.path().join("notes/Note.md")).expect("remove");
        assert!(matches!(
            resolver.resolve("Note", MAX_EMBED_DEPTH, &[]),
            EmbedResolution::Cut { .. }
        ));
    }

    #[test]
    fn a_note_the_index_has_but_the_disk_no_longer_does_is_missing() {
        let (dir, resolver) = fixture(local);
        fs::remove_file(dir.path().join("notes/Note.md")).expect("remove");
        assert!(matches!(
            resolver.resolve("Note", 0, &[]),
            EmbedResolution::Missing
        ));
    }

    #[test]
    fn a_note_past_the_size_ceiling_is_cut_rather_than_rendered() {
        let (dir, resolver) = fixture(local);
        let big = "x".repeat(MAX_EMBED_BYTES as usize + 1);
        fs::write(dir.path().join("notes/Note.md"), big).expect("write");
        assert!(matches!(
            resolver.resolve("Note", 0, &[]),
            EmbedResolution::Cut { .. }
        ));
    }
}
