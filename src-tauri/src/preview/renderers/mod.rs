//! First-party content-renderer registration entry point — ADR-009 §D1.
//!
//! Registration order tracks the renderer roster (lean scope):
//!
//! 1. `html` (L2)
//! 2. `markdown` (L4) — also rewrites ```mermaid fences and (L6) math
//! 3. `mermaid` (L5 — standalone `.mmd` diagrams)

use writ_core::preview::{ContentRendererRegistry, RegisterError};

pub mod html;
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
pub fn register_builtins(registry: &mut ContentRendererRegistry) -> Result<(), RegisterError> {
    registry.register(Box::new(HtmlRenderer))?;
    registry.register(Box::new(MarkdownRenderer))?;
    registry.register(Box::new(MermaidRenderer))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::preview::ContentTypeId;

    #[test]
    fn register_builtins_registers_all_renderers() {
        let mut reg = ContentRendererRegistry::new();
        register_builtins(&mut reg).unwrap();
        assert!(reg.has(&ContentTypeId::new("html")));
        assert!(reg.has(&ContentTypeId::new("markdown")));
        assert!(reg.has(&ContentTypeId::new("mermaid")));
        assert_eq!(reg.len(), 3);
    }
}
