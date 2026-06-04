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
//! There is no per-document trust policy. The preview renders the user's
//! own offline agent output under one fixed CSP; network is categorically
//! off, and the only variable is an app-level scripts kill switch applied
//! at serve time (see the ADR-010/011 supersede notes). [`ContentRenderer`]
//! mirrors ADR-006's loader-agnostic registry precedent.

pub mod protocol;
pub mod registry;
pub mod types;

pub use protocol::{parse as parse_preview_url, ParsedRequest, PreviewScope, RefusalReason};
pub use registry::{ContentRendererRegistry, RegisterError};
pub use types::{
    ContentRenderer, ContentTypeId, LayoutMode, RenderError, RenderOutput, RenderRequest,
    RendererCapabilities, ViewMode, WindowId,
};
