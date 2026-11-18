//! Reading a file's identity from the platform.
//!
//! The reading lives in [`jotra_storage::identity`] with the rest of the disk
//! I/O, so `jotra mcp` keys the versions it keeps the way the app does.

pub use jotra_storage::identity::{read_identity, PlatformIdentity};
