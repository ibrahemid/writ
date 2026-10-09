//! Reading a file's identity from the platform.
//!
//! The reading lives in [`writ_storage::identity`] with the rest of the disk
//! I/O, so `writ mcp` keys the versions it keeps the way the app does.

pub use writ_storage::identity::{read_identity, PlatformIdentity};
