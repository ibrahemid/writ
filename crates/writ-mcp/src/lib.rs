//! The MCP server other programs read a Writ notes folder through.
//!
//! Three modules and one boundary. [`tools`] is the surface in plain Rust,
//! [`consent`] decides who may reach it, and [`server`] is the only file that
//! names `rmcp`, so an SDK bump is one file (ADR-031 section 1, ADR-032
//! section 8).
//!
//! The crate imports no tauri and no HTTP client, asserted by
//! `tests/no_tauri_dependency.rs`: the server speaks stdio to a process the
//! user launched and makes no outbound request of its own (ADR-031 rule 2.3).

/// Who may call a tool.
pub mod consent;
/// The protocol.
pub mod server;
/// The tool surface.
pub mod tools;

pub use consent::{ClientId, ConsentGate, Decision};
pub use tools::{ToolError, ToolHost};
