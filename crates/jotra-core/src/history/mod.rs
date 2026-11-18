//! Recently-closed buffer history.
//!
//! Closed buffers are retained in history until they exceed
//! [`crate::config::HistoryConfig::max_entries`]. The history layer is
//! the source of truth for the "reopen closed tab" shortcut.

/// Operations applied to the history stack (reserved).
pub mod operations;
