//! The file a document Writ generates itself has to have.
//!
//! The third-party notices listing, and anything else titled but never typed
//! by the user, must never mint a file in the notes folder: that folder holds
//! only what the user wrote (ADR-028 §1). Each such document instead gets a
//! fixed path under the data directory, one per title, that reopening never
//! grows: a second open rewrites the same file rather than minting a
//! `" 2"` sibling the way [`crate::notes::mint_note_path`] would.

use std::path::{Path, PathBuf};

/// The path a generated document titled `title` always writes to, inside
/// `writ_dir`.
pub fn generated_document_path(writ_dir: &Path, title: &str) -> PathBuf {
    let stem = writ_core::notes::sanitize_title_or(title, "generated-document");
    writ_dir.join("generated").join(format!("{stem}.md"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_path_sits_under_generated_named_for_the_title() {
        let root = Path::new("/writ");
        let path = generated_document_path(root, "Third-party licences");
        assert_eq!(path, Path::new("/writ/generated/Third-party licences.md"));
    }

    #[test]
    fn reopening_the_same_title_yields_the_same_path() {
        let root = Path::new("/writ");
        let first = generated_document_path(root, "Third-party licences");
        let second = generated_document_path(root, "Third-party licences");
        assert_eq!(first, second, "a second open must land on the same file");
    }

    #[test]
    fn an_unsanitary_title_still_yields_a_path() {
        let root = Path::new("/writ");
        let path = generated_document_path(root, "a/b");
        assert_eq!(path, Path::new("/writ/generated/a b.md"));
    }

    #[test]
    fn an_empty_title_falls_back_rather_than_yielding_a_bare_extension() {
        let root = Path::new("/writ");
        let path = generated_document_path(root, "   ");
        assert_eq!(path, Path::new("/writ/generated/generated-document.md"));
    }
}
