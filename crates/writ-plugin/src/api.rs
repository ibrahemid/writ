use writ_core::buffer::document::BufferDocument;

pub trait PluginApi {
    fn get_active_buffers(&self) -> Vec<BufferDocument>;
    fn get_buffer_content(&self, id: &str) -> Option<String>;
    fn create_buffer(&self, title: &str, content: &str) -> String;
}
