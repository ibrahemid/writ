//! Preview substrate — ADR-009.
//!
//! This module owns:
//!
//! - [`protocol`] — `writ-preview://` URL parser, scope routing, refusal
//!   logic, and the debug-only request recorder hook the Phase-3
//!   verification suite reads.
//! - [`csp`] — per-webview CSP construction. Phase 1 emits the default-deny
//!   baseline; Phase 3 populates the per-cell matrix from ADR-011.
//! - [`webview_manager`] — [`webview_manager::PreviewWebviewManager`],
//!   the warm-pool + per-tab webview lifecycle controller.
//! - [`window_manager`] — [`window_manager::WindowManager`], the
//!   [`writ_core::preview::WindowId`] ↔ Tauri `WebviewWindow` registry.
//! - [`renderers`] — registration entry point for first-party content
//!   renderers. Phase 1 ships an empty registration; later phases plug in
//!   HTML, markdown, Mermaid, math, PDF, SVG, raster image.

pub mod csp;
pub mod handler;
pub mod protocol;
pub mod renderers;
pub mod webview_manager;
pub mod window_manager;
