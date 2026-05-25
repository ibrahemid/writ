//! Preview surface domain types and content-type renderer registry.
//!
//! `writ-core::preview` is the pure-domain core of the preview epic (ADR-009).
//! It owns the [`ContentRenderer`] trait, the [`ContentRendererRegistry`],
//! the [`LayoutMode`] / [`ViewMode`] state types, the [`RenderRequest`] /
//! [`RenderOutput`] / [`RenderError`] payload shapes, and the
//! [`PreviewPolicy`] enum.
//!
//! # Boundary
//!
//! Zero Tauri dependency. The webview lifecycle, the `writ-preview://`
//! protocol handler, the per-platform CSP wiring, and the renderer-instance
//! registration all live in `src-tauri`. This module commits the shapes
//! `src-tauri` plugs into.
//!
//! # Scope per ADR
//!
//! - [`PreviewPolicy`] is declared here so `ContentRenderer::render` is
//!   callable from a pure-domain test without Tauri. ADR-011 populates the
//!   per-state CSP bytes; ADR-009 commits only the four-variant shape.
//! - [`ContentRenderer`] mirrors ADR-006's loader-agnostic registry
//!   precedent: first-party renderers and a future external renderer adapter
//!   plug into the same trait.

pub mod registry;
pub mod types;

pub use registry::{ContentRendererRegistry, RegisterError};
pub use types::{
    ContentRenderer, ContentTypeId, LayoutMode, PreviewPolicy, RenderError, RenderOutput,
    RenderRequest, RendererCapabilities, ViewMode, WindowId,
};
