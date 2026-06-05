//! First-party content-renderer registration entry point — ADR-009 §D1.
//!
//! Registration order tracks the renderer roster (lean scope):
//!
//! 1. `html` (L2)
//! 2. `markdown` (L4 — this module)
//! 3. `mermaid` (L5), `math` (L6)

use writ_core::preview::{ContentRendererRegistry, RegisterError};

pub mod html;
pub mod markdown;
pub mod theme;

pub use html::HtmlRenderer;
pub use markdown::MarkdownRenderer;

/// Register every first-party renderer into the supplied registry.
///
/// Each later phase appends a registration line and ships a renderer
/// module alongside.
pub fn register_builtins(registry: &mut ContentRendererRegistry) -> Result<(), RegisterError> {
    registry.register(Box::new(HtmlRenderer))?;
    registry.register(Box::new(MarkdownRenderer))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::preview::ContentTypeId;

    #[test]
    fn register_builtins_registers_html_and_markdown() {
        let mut reg = ContentRendererRegistry::new();
        register_builtins(&mut reg).unwrap();
        assert!(reg.has(&ContentTypeId::new("html")));
        assert!(reg.has(&ContentTypeId::new("markdown")));
        assert_eq!(reg.len(), 2);
    }
}
