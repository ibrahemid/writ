//! Plugin surface for the Writ editor.
//!
//! `writ-plugin` defines the extension boundary as a pair of lightweight,
//! dependency-light modules:
//!
//! - [`manifest`] declares the shape of a plugin's metadata file.
//! - [`api`] declares the trait that the host exposes to plugin code.
//!
//! Everything here is intentionally stable and free of runtime concerns so
//! that plugins can target this crate without pulling in storage, Tauri, or
//! other host-side dependencies.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// Host-side API surface exposed to plugins.
pub mod api;
/// Metadata declared by a Writ plugin on disk.
pub mod manifest;
/// Text-transform trait, registry, and built-in transforms.
pub mod transform;
