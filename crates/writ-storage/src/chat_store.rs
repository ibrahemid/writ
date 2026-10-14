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
//!
//! The list is drawn from `chats/index.json`, which holds each conversation's
//! title, dates and turn count beside the size and modification time its file
//! had when the row was written. Listing reads the folder's metadata and the
//! index, and opens a conversation only when its file has no row or no longer
//! matches it, so a list costs one small file however long the history is.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::UNIX_EPOCH;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use writ_core::chat::{Conversation, MAX_CONVERSATION_BYTES};

use crate::atomic::{write_atomic, AtomicWriteError};

/// The folder conversations live in, under the data directory.
const CHATS_DIR: &str = "chats";

/// The extension every conversation file carries.
const CHAT_EXTENSION: &str = "json";

/// The index of the folder. Its stem is not a UUID, so it is never read as a
/// conversation.
const INDEX_FILE: &str = "index.json";

/// The schema the index is written at. An index at another version is read
/// as no index, and rebuilt.
const INDEX_VERSION: u32 = 1;

/// Held across every write of a conversation and its index row, and across a
/// list's read and rewrite of the index.
///
/// Every command builds its own [`ChatStore`], and a reply's task saves from
/// its own thread, so a lock on the value would order nothing. Holding the
/// conversation's write under the same lock is what makes the size and time a
/// row records belong to the summary beside them.
static INDEX_LOCK: Mutex<()> = Mutex::new(());

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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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

/// What a conversation's file looked like when its row was written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
struct FileStamp {
    /// Its length in bytes.
    bytes: u64,
    /// Its modification time, in nanoseconds since the epoch.
    modified: u64,
}

impl FileStamp {
    fn of(meta: &fs::Metadata) -> Self {
        let modified = meta
            .modified()
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .map_or(0, |since| {
                u64::try_from(since.as_nanos()).unwrap_or(u64::MAX)
            });
        Self {
            bytes: meta.len(),
            modified,
        }
    }
}

/// One row of the index: the summary, and the file it was read from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct IndexRow {
    summary: ConversationSummary,
    stamp: FileStamp,
}

/// `chats/index.json`, keyed by conversation id.
#[derive(Debug, Default, Serialize, Deserialize)]
struct Index {
    version: u32,
    rows: BTreeMap<String, IndexRow>,
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
    /// been written. Rows come from the index; a file the index has no row
    /// for, or whose size or time moved since its row was written, is read
    /// and its row rewritten, and a row whose file is gone is dropped. A file
    /// that does not parse is skipped with a warning naming the file, so one
    /// bad file does not hide the rest.
    pub fn list(&self) -> Result<Vec<ConversationSummary>, ChatStoreError> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };

        let mut files: Vec<(String, PathBuf, FileStamp)> = Vec::new();
        for entry in entries {
            let entry = entry?;
            let path = entry.path();
            let Some(id) = conversation_id_of(&path) else {
                continue;
            };
            let meta = entry.metadata()?;
            if !meta.is_file() {
                continue;
            }
            files.push((id, path, FileStamp::of(&meta)));
        }

        let _held = INDEX_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let mut index = self.read_index();
        let mut changed = false;
        let mut summaries: Vec<ConversationSummary> = Vec::with_capacity(files.len());
        for (id, path, stamp) in &files {
            if let Some(row) = index.rows.get(id).filter(|row| row.stamp == *stamp) {
                summaries.push(row.summary.clone());
                continue;
            }
            changed = true;
            match fs::read(path)
                .map_err(ChatStoreError::from)
                .and_then(|bytes| {
                    serde_json::from_slice::<Conversation>(&bytes).map_err(ChatStoreError::from)
                }) {
                Ok(conversation) => {
                    let summary = ConversationSummary::from(&conversation);
                    index.rows.insert(
                        id.clone(),
                        IndexRow {
                            summary: summary.clone(),
                            stamp: *stamp,
                        },
                    );
                    summaries.push(summary);
                }
                // The file name, and nothing the file holds: what failed to
                // parse is turn text.
                Err(error) => {
                    index.rows.remove(id);
                    tracing::warn!(
                        file = %file_name(path),
                        kind = error_kind(&error),
                        "skipping a file that does not hold a conversation"
                    );
                }
            }
        }
        let before = index.rows.len();
        index
            .rows
            .retain(|id, _| files.iter().any(|(listed, _, _)| listed == id));
        changed |= index.rows.len() != before;
        if changed {
            self.write_index(&index);
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
        let _held = INDEX_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write_atomic(&file, &bytes)?;
        // A row that cannot be written leaves the old one, which no longer
        // matches the file, so the next list reads this conversation instead.
        match fs::metadata(&file) {
            Ok(meta) => {
                let mut index = self.read_index();
                index.rows.insert(
                    conversation.id.clone(),
                    IndexRow {
                        summary: ConversationSummary::from(conversation),
                        stamp: FileStamp::of(&meta),
                    },
                );
                self.write_index(&index);
            }
            Err(error) => tracing::warn!(
                kind = %error.kind(),
                "the saved conversation could not be measured for the index"
            ),
        }
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
        let _held = INDEX_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match fs::remove_file(&file) {
            Ok(()) => {
                let mut index = self.read_index();
                if let Some(name) = file.file_stem().and_then(|stem| stem.to_str()) {
                    if index.rows.remove(name).is_some() {
                        self.write_index(&index);
                    }
                }
                Ok(())
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                Err(ChatStoreError::NotFound(id.to_string()))
            }
            Err(error) => Err(error.into()),
        }
    }

    /// The index as it stands, or an empty one when it is missing, unreadable
    /// or written at another version. Called with [`INDEX_LOCK`] held.
    fn read_index(&self) -> Index {
        let read = fs::read(self.dir.join(INDEX_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Index>(&bytes).ok());
        match read {
            Some(index) if index.version == INDEX_VERSION => index,
            _ => Index {
                version: INDEX_VERSION,
                rows: BTreeMap::new(),
            },
        }
    }

    /// Replaces the index. A failure is a warning: the conversations are
    /// whole, and the next list rebuilds what the index lost. Called with
    /// [`INDEX_LOCK`] held.
    fn write_index(&self, index: &Index) {
        let written = serde_json::to_vec(index)
            .map_err(ChatStoreError::from)
            .and_then(|bytes| {
                fs::create_dir_all(&self.dir)?;
                write_atomic(&self.dir.join(INDEX_FILE), &bytes).map_err(ChatStoreError::from)
            });
        if let Err(error) = written {
            tracing::warn!(
                kind = error_kind(&error),
                "the conversation index was not written"
            );
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

/// The id a conversation file is named by, or `None` for anything else in the
/// folder: the index, a temporary file, a name that is not a UUID in the form
/// [`ChatStore`] writes.
fn conversation_id_of(path: &Path) -> Option<String> {
    if path.extension().and_then(|ext| ext.to_str()) != Some(CHAT_EXTENSION) {
        return None;
    }
    let stem = path.file_stem()?.to_str()?;
    let parsed = uuid::Uuid::parse_str(stem).ok()?.to_string();
    (parsed == stem).then_some(parsed)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The one word a skipped file's warning says about why, with nothing the file
/// held in it.
fn error_kind(error: &ChatStoreError) -> &'static str {
    match error {
        ChatStoreError::Json(_) => "json",
        _ => "io",
    }
}
