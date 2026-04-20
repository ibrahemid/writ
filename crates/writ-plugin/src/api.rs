use writ_core::buffer::document::BufferDocument;

/// Host-side API surface exposed to plugins.
///
/// Implementations of this trait are provided by the Writ host and give
/// plugins a narrow, intentionally read-biased view of the editor state.
/// Keeping the surface small is deliberate: every addition widens the
/// compatibility contract plugins rely on.
pub trait PluginApi {
    /// Returns documents for every buffer currently open as a tab.
    fn get_active_buffers(&self) -> Vec<BufferDocument>;

    /// Returns the textual content of a buffer by id, or `None` when the
    /// buffer is unknown or its backing content cannot be read.
    fn get_buffer_content(&self, id: &str) -> Option<String>;

    /// Creates a new buffer with the given title and initial content and
    /// returns the newly assigned buffer id.
    fn create_buffer(&self, title: &str, content: &str) -> String;
}
