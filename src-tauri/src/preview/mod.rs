//! Preview substrate — ADR-009 (lean re-scope).
//!
//! The split/swap preview is a same-window `<iframe>` over `writ-preview://`,
//! not a child Tauri webview — see the ADR-009 lean amendment. The security
//! boundary is the response-header CSP the protocol handler attaches, which
//! is identical for an iframe and a child webview. This module owns:
//!
//! - [`protocol`] — `writ-preview://` URL parser, scope routing, refusal
//!   logic, and the debug-only request recorder the exfil-denial suite reads.
//! - [`csp`] — the fixed document CSP (+ scripts kill switch) and the
//!   chrome-scope CSP.
//! - [`handler`] — request resolution, the render cache, and the Tauri
//!   protocol glue that attaches the CSP header to every response.
//! - [`renderers`] — registration entry point for first-party content
//!   renderers (HTML now; markdown / Mermaid / KaTeX next).

pub mod csp;
pub mod csp_eval;
pub mod handler;
pub mod protocol;
pub mod renderers;
