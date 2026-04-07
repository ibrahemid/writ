use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BufferStatus {
    Active,
    History,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BufferDocument {
    pub id: String,
    pub title: String,
    pub filename: String,
    pub status: BufferStatus,
    pub language: Option<String>,
    pub source_path: Option<String>,
    pub cursor_pos: u64,
    pub scroll_pos: u64,
    pub tab_order: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub closed_at: Option<DateTime<Utc>>,
}
