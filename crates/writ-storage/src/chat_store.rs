//! Conversation files under the data directory (ADR-040 section 8).
//!
//! One conversation is one JSON file at `<writ_dir>/chats/<uuid>.json`. The
//! folder is never inside the notes folder, so nothing here is a note and
//! nothing here is swept by the index.
//!
//! What the store holds is [`writ_core::chat::Conversation`]: the turns, the
//! proposals with their status, and attachments by path, size and digest. It
//! holds no API key and no attached note's text, which the crate's
//! `chat_store_tests` prove by walking the written file's keys.
//!
//! An id is parsed as a UUID and the file is named by the hyphenated form it
//! prints as, so no caller can name a file outside the folder and no spelling
//! of an id names a second file. Every write goes through
//! [`crate::atomic::write_atomic`], so a conversation is never half a file.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use thiserror::Error;
use writ_core::chat::{Conversation, MAX_CONVERSATION_BYTES};

use crate::atomic::{write_atomic, AtomicWriteError};

/// The folder conversations live in, under the data directory.
const CHATS_DIR: &str = "chats";

/// The extension every conversation file carries.
const CHAT_EXTENSION: &str = "json";

/// Why a conversation was not read or written.
///
/// Every variant names an id or a count. None of them can carry a turn's text,
/// a note's text or a key, because a message a person reads is the one place
/// such a string could reach a log (ADR-031 rule 5.2).
#[derive(Debug, Error)]
pub enum ChatStoreError {
    /// No file is stored under that id.
    #[error("no conversation is stored under {0}")]
    NotFound(String),

    /// The conversation would be larger than a file may hold.
    #[error("the conversation is {bytes} bytes, over the {limit} one may hold")]
    TooLarge {
        /// What the document serialized to.
        bytes: usize,
        /// [`MAX_CONVERSATION_BYTES`].
        limit: usize,
    },

    /// The id is not a UUID, so it names no conversation and no path is built
    /// from it.
    #[error("that is not the id of a conversation")]
    InvalidId(String),

    /// The filesystem refused.
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    /// The file does not hold a conversation.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The replace a save performs did not happen.
    #[error("write error: {0}")]
    Write(#[from] AtomicWriteError),
}

/// One row of the conversation list.
///
/// The turns themselves stay on disk: the list is shown before a conversation
/// is opened, and reading every turn of every file to draw it would grow with
/// the history rather than with the list.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ConversationSummary {
    /// The conversation's id, which is also its file name.
    pub id: String,
    /// What the pane calls it.
    pub title: String,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it last changed, RFC 3339.
    pub updated_at: String,
    /// How many turns it holds.
    pub turns: usize,
}

impl From<&Conversation> for ConversationSummary {
    fn from(conversation: &Conversation) -> Self {
        Self {
            id: conversation.id.clone(),
            title: conversation.title.clone(),
            created_at: conversation.created_at.clone(),
            updated_at: conversation.updated_at.clone(),
            turns: conversation.turns.len(),
        }
    }
}

/// The conversation folder under one data directory.
#[derive(Debug, Clone)]
pub struct ChatStore {
    dir: PathBuf,
}

impl ChatStore {
    /// The store under `<writ_dir>/chats`. The folder is created by the first
    /// write, so opening a store touches no disk.
    pub fn new(writ_dir: &Path) -> Self {
        Self {
            dir: writ_dir.join(CHATS_DIR),
        }
    }

    /// The moment a change is stamped with, RFC 3339.
    pub fn now() -> String {
        chrono::Utc::now().to_rfc3339()
    }

    /// A new, empty conversation, on disk before it is handed back.
    ///
    /// Saving at once is what makes the id the frontend streams under an id
    /// the store already knows: a send that arrives first would otherwise have
    /// no file to append its turn to.
    pub fn create(&self, provider: &str, model: &str) -> Result<Conversation, ChatStoreError> {
        let conversation = Conversation::new(
            uuid::Uuid::new_v4().to_string(),
            Self::now(),
            provider.to_string(),
            model.to_string(),
        );
        self.save(&conversation)?;
        Ok(conversation)
    }

    /// Every conversation the folder holds, most recently changed first.
    ///
    /// A folder that does not exist yet is an empty list rather than an error:
    /// the pane asks for the list the first time it opens, before anything has
    /// been written. A file that does not parse is skipped with a warning
    /// naming the file, so one bad file does not hide the rest.
    pub fn list(&self) -> Result<Vec<ConversationSummary>, ChatStoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        let mut summaries: Vec<ConversationSummary> = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some(CHAT_EXTENSION) {
                continue;
            }
            match fs::read(&path)
                .map_err(ChatStoreError::from)
                .and_then(|bytes| {
                    serde_json::from_slice::<Conversation>(&bytes).map_err(ChatStoreError::from)
                }) {
                Ok(conversation) => summaries.push(ConversationSummary::from(&conversation)),
                // The file name, and nothing the file holds: what failed to
                // parse is turn text.
                Err(error) => tracing::warn!(
                    file = %entry.file_name().to_string_lossy(),
                    kind = error_kind(&error),
                    "skipping a file that does not hold a conversation"
                ),
            }
        }

        summaries.sort_by(|left, right| {
            right
                .updated_at
                .cmp(&left.updated_at)
                .then_with(|| left.id.cmp(&right.id))
        });
        Ok(summaries)
    }

    /// The conversation stored under `id`.
    pub fn load(&self, id: &str) -> Result<Conversation, ChatStoreError> {
        let file = self.file(id)?;
        let bytes = match fs::read(&file) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(ChatStoreError::NotFound(id.to_string()))
            }
            Err(error) => return Err(error.into()),
        };
        Ok(serde_json::from_slice(&bytes)?)
    }

    /// Replaces the conversation's file.
    ///
    /// The cap is checked against the bytes that would be written rather than
    /// against a count of turns, because it is the file that has to stay
    /// readable. A refusal leaves whatever is on disk exactly as it was.
    pub fn save(&self, conversation: &Conversation) -> Result<(), ChatStoreError> {
        let file = self.file(&conversation.id)?;
        let bytes = serde_json::to_vec_pretty(conversation)?;
        if bytes.len() > MAX_CONVERSATION_BYTES {
            return Err(ChatStoreError::TooLarge {
                bytes: bytes.len(),
                limit: MAX_CONVERSATION_BYTES,
            });
        }
        fs::create_dir_all(&self.dir)?;
        write_atomic(&file, &bytes)?;
        Ok(())
    }

    /// Names the conversation, keeping the name it has when the new one holds
    /// nothing. The title is the user's, so it is not cut the way a derived
    /// one is.
    pub fn rename(&self, id: &str, title: &str) -> Result<Conversation, ChatStoreError> {
        let mut conversation = self.load(id)?;
        let trimmed = title.trim();
        if !trimmed.is_empty() {
            conversation.title = trimmed.to_string();
        }
        conversation.updated_at = Self::now();
        self.save(&conversation)?;
        Ok(conversation)
    }

    /// Unlinks the conversation's file. There is no trash for a conversation,
    /// because it is not a note.
    pub fn delete(&self, id: &str) -> Result<(), ChatStoreError> {
        let file = self.file(id)?;
        match fs::remove_file(&file) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ChatStoreError::NotFound(id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// The file an id names, refusing an id that is not a UUID.
    ///
    /// The name is built from the hyphenated form the parsed value prints as,
    /// never from the string the caller wrote, which `Uuid::parse_str` also
    /// accepts braced, as a URN and unhyphenated. So the four spellings of one
    /// id name one file, and the name holds no separator, no dot, no colon and
    /// no drive letter: a path built from it cannot leave the folder.
    fn file(&self, id: &str) -> Result<PathBuf, ChatStoreError> {
        let name = uuid::Uuid::parse_str(id)
            .map_err(|_| ChatStoreError::InvalidId(id.to_string()))?
            .to_string();
        Ok(self.dir.join(format!("{name}.{CHAT_EXTENSION}")))
    }
}

/// The one word a skipped file's warning says about why, with nothing the file
/// held in it.
fn error_kind(error: &ChatStoreError) -> &'static str {
    match error {
        ChatStoreError::Json(_) => "json",
        _ => "io",
    }
}
