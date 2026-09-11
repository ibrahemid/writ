//! Extension surface for the Writ editor.
//!
//! Two modules, neither of which loads anything from disk:
//!
//! - [`host`] is the capability-scoped surface a program reaches a whole note
//!   through, re-exported from `writ-core` so a consumer finds the extension
//!   boundary in one crate (ADR-032 section 8).
//! - [`transform`] is the in-process text-transform trait, its registry and the
//!   built-ins (ADR-006, ADR-012). A transform sees a string; a host call sees
//!   a note.
//!
//! Nothing here is user-installable and no third-party code is loaded. The
//! surface is internal to Writ's own binary and carries no compatibility
//! guarantee, because it has no caller outside this tree.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

pub use writ_core::notes::host;

/// Text-transform trait, registry, and built-in transforms.
pub mod transform;
