//! First-party content-renderer registration entry point — ADR-009 §D1.
//!
//! Registration order tracks the renderer roster (lean scope):
//!
//! 1. `html` (L2)
//! 2. `markdown` (L4) — also rewrites ```mermaid fences (L5) and math (L6)
//! 3. `mermaid` (L5 — standalone `.mmd` diagrams)
//!
//! `katex` (L6) carries no standalone content type: KaTeX typesets math
//! expressions, not LaTeX documents, so a `.tex` file is not a renderable
//! unit. It lives only as Markdown math enhancement + the shared asset table.

use std::sync::Arc;

use writ_core::preview::{ContentRendererRegistry, RegisterError};
use writ_storage::notes_index::NotesIndexStore;

pub mod html;
pub mod katex;
pub mod markdown;
pub mod mermaid;
pub mod theme;

pub use html::HtmlRenderer;
pub use markdown::MarkdownRenderer;
pub use mermaid::MermaidRenderer;

/// Register every first-party renderer into the supplied registry.
///
/// Each later phase appends a registration line and ships a renderer
/// module alongside.
///
/// The Markdown renderer is handed the notes index because a `[[…]]` means
/// whatever the index says it means; the render happens here, not in the
/// protocol handler, which serves the HTML this produced.
pub fn register_builtins(
    registry: &mut ContentRendererRegistry,
    notes_index: Arc<NotesIndexStore>,
) -> Result<(), RegisterError> {
    registry.register(Box::new(HtmlRenderer))?;
    registry.register(Box::new(MarkdownRenderer::new(notes_index)))?;
    registry.register(Box::new(MermaidRenderer))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::preview::ContentTypeId;

    #[test]
    fn register_builtins_registers_all_renderers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("writ.db");
        let conn = writ_storage::database::connection::open_database(&db_path).expect("open");
        writ_storage::database::migrations::run_migrations(&conn).expect("migrations");
        drop(conn);
        let index = Arc::new(NotesIndexStore::open(&db_path).expect("index"));
        let mut reg = ContentRendererRegistry::new();
        register_builtins(&mut reg, index).unwrap();
        assert!(reg.has(&ContentTypeId::new("html")));
        assert!(reg.has(&ContentTypeId::new("markdown")));
        assert!(reg.has(&ContentTypeId::new("mermaid")));
        assert_eq!(reg.len(), 3);
    }
}
