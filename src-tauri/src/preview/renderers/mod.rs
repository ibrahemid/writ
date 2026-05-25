//! First-party content-renderer registration entry point — ADR-009 §D1.
//!
//! Phase 1 ships the wiring (`register_builtins`) with an empty registration
//! list. Phase 2 plugs in [`writ_core::preview::ContentRenderer`] impls in
//! the order:
//!
//! 1. `html` (Phase 2)
//! 2. `markdown`, `mermaid`, `math`, `pdf`, `svg`, `image` (Phase 5,
//!    one ADR per engine)

use writ_core::preview::{ContentRendererRegistry, RegisterError};

/// Register every first-party renderer into the supplied registry.
///
/// Phase 1: this is a no-op so the registry surface is callable at
/// startup. Each later phase appends a registration line and ships a
/// renderer module alongside.
pub fn register_builtins(_registry: &mut ContentRendererRegistry) -> Result<(), RegisterError> {
    // No renderers in Phase 1 — see module-level doc.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_builtins_succeeds_with_empty_registry() {
        let mut reg = ContentRendererRegistry::new();
        register_builtins(&mut reg).unwrap();
        assert!(reg.is_empty());
    }
}
