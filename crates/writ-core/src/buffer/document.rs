use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Lifecycle state of a buffer.
///
/// A buffer is either `Active` (visible in the tab strip) or `History`
/// (closed but retained for the reopen-closed-tab shortcut).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BufferStatus {
    /// Buffer is open as a tab.
    Active,
    /// Buffer has been closed but is retained in history.
    History,
}

/// A single buffer's metadata.
///
/// Content bytes live on disk; this struct carries the identity, title,
/// lifecycle state, cursor position, and timestamps used by the rest of
/// the editor. Buffers are serialized in events and in storage without
/// their content.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferDocument {
    /// Stable UUID assigned at creation time.
    pub id: String,
    /// User-visible title shown on the tab.
    pub title: String,
    /// On-disk filename under the buffers directory.
    pub filename: String,
    /// Current lifecycle state.
    pub status: BufferStatus,
    /// Detected or user-assigned language identifier (for example `rust`).
    pub language: Option<String>,
    /// Absolute path to the originating file, if the buffer mirrors one.
    pub source_path: Option<String>,
    /// Cursor offset in bytes from the start of the buffer.
    pub cursor_pos: u64,
    /// Scroll offset in bytes from the start of the buffer.
    pub scroll_pos: u64,
    /// Position of the tab in the visible tab strip; lower is earlier.
    pub tab_order: u32,
    /// Timestamp the buffer was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the last metadata or content update.
    pub updated_at: DateTime<Utc>,
    /// Timestamp the buffer was moved to history, if applicable.
    pub closed_at: Option<DateTime<Utc>>,
    /// When `true` the buffer must never be written back to its source.
    ///
    /// Set for binary (hex-view) buffers. Enforced server-side in
    /// `save_buffer_content` and `save_to_source`.
    #[serde(default)]
    pub read_only: bool,
    /// Size in bytes of the source file at open time. Used to re-derive the
    /// large-file mode tier on tab-switch without a second `stat` call.
    #[serde(default)]
    pub size_bytes: u64,
}
