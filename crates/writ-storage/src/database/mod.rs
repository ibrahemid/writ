//! Raw database primitives.
//!
//! This module wraps `rusqlite` with WAL-mode connection setup, idempotent
//! migration execution, and hand-written SQL statements. Higher-level
//! stores build on top of these primitives.

/// Connection opening with Writ's pragma defaults.
pub mod connection;
/// Schema migration runner and embedded SQL.
pub mod migrations;
/// Hand-written SQL statements for buffer persistence.
pub mod queries;
