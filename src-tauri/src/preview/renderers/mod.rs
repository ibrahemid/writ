//! First-party content-renderer registration entry point — ADR-009 §D1.
//!
//! Registration order tracks the renderer roster:
//!
//! 1. `html` (Phase 2 — this module)
//! 2. `markdown`, `mermaid`, `math`, `pdf`, `svg`, `image` (Phase 5,
//!    one ADR per engine)

use writ_core::preview::{ContentRendererRegistry, RegisterError};

pub mod html;

pub use html::HtmlRenderer;

/// Register every first-party renderer into the supplied registry.
///
/// Each later phase appends a registration line and ships a renderer
/// module alongside.
pub fn register_builtins(registry: &mut ContentRendererRegistry) -> Result<(), RegisterError> {
    registry.register(Box::new(HtmlRenderer))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use writ_core::preview::ContentTypeId;

    #[test]
    fn register_builtins_registers_html() {
        let mut reg = ContentRendererRegistry::new();
        register_builtins(&mut reg).unwrap();
        assert!(reg.has(&ContentTypeId::new("html")));
        assert_eq!(reg.len(), 1);
    }
}
