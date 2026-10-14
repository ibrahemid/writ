//! Command registry types for palette-driven actions.
//!
//! Writ exposes user-visible actions through a central registry that the
//! command palette, keyboard handler, and plugins all consume.

/// Command registry primitives.
pub mod registry;
/// How often each command ran, which ranks the palette.
pub mod usage;
