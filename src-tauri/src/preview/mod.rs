//! Preview substrate — ADR-009 (lean re-scope).
//!
//! The split/swap preview is a same-window `<iframe>` over `jotra-preview://`,
//! not a child Tauri webview — see the ADR-009 lean amendment. The security
//! boundary is the response-header CSP the protocol handler attaches, which
//! is identical for an iframe and a child webview. This module owns:
//!
//! - [`protocol`] — `jotra-preview://` URL parser, scope routing, refusal
//!   logic, and the debug-only request recorder the exfil-denial suite reads.
//! - [`csp`] — the document CSP for each platform, whose `script-src`
//!   follows the per-file scripts opt-in, and the chrome-scope CSP.
//! - [`handler`] — request resolution, the render cache, and the Tauri
//!   protocol glue that attaches the CSP header to every response and
//!   disables a served document's resource hints and frame navigations
//!   (ADR-043).
//! - [`remote_fetch`] — the one GET behind a remote image the editor was
//!   asked to show (ADR-057).
//! - [`renderers`] — registration entry point for first-party content
//!   renderers (HTML now; markdown / Mermaid / KaTeX next).

pub mod csp;
pub mod embeds;
pub mod handler;
#[cfg(test)]
pub mod log_capture;
pub mod protocol;
pub mod remote_fetch;
pub mod renderers;
pub mod wikilinks;
