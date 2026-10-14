//! Preview surface domain types and content-type renderer registry.
//!
//! `writ-core::preview` is the pure-domain core of the preview epic (ADR-009).
//! It owns the [`ContentRenderer`] trait, the [`ContentRendererRegistry`],
//! the [`LayoutMode`] / [`ViewMode`] state types, and the [`RenderRequest`]
//! / [`RenderOutput`] / [`RenderError`] payload shapes.
//!
//! # Boundary
//!
//! Zero Tauri dependency. The webview lifecycle, the `writ-preview://`
//! protocol handler, the CSP wiring, and the renderer-instance registration
//! all live in `src-tauri`. This module commits the shapes `src-tauri`
//! plugs into.
//!
//! # Lean scope
//!
//! There is no trust matrix. Every document is served under one fixed CSP
//! with the network off; the one variable is a per-file opt-in that lets a
//! document's own scripts run (ADR-043), which [`scripts`] says when to take
//! back. Before a document is served,
//! [`hints`] disables the resource hints it carries and [`frames`] disarms the
//! frame navigations it would start without a script. [`ContentRenderer`]
//! mirrors ADR-006's loader-agnostic registry precedent.

mod attributes;
pub mod frames;
pub mod hints;
pub mod inline_image;
pub mod protocol;
pub mod registry;
pub mod scripts;
pub mod types;

pub use inline_image::{
    check_inline_image_head, check_inline_image_stat, inline_image_route, resolve_inline_image,
    InlineImageRefusal,
};
pub use protocol::{
    parse as parse_preview_url, AssetReference, AssetRequest, AssetRoot, ParsedRequest,
    PreviewScope, RefusalReason,
};
pub use registry::{render_contained, ContentRendererRegistry, RegisterError, RENDERER_PANICKED};
pub use types::{
    AssetScope, ContentRenderer, ContentTypeId, LayoutMode, RenderError, RenderOutput,
    RenderRequest, RendererCapabilities, ThemePolarity, ViewMode, WindowId,
};
