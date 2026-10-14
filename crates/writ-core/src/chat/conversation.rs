//! The document one conversation is saved as (ADR-040 section 8).

use serde::{Deserialize, Serialize};

use super::*;

/// The schema every conversation file is written at (ADR-040 section 8).
pub const CONVERSATION_SCHEMA_VERSION: u32 = 1;

/// The largest a conversation file may become before a send is refused.
pub const MAX_CONVERSATION_BYTES: usize = 4 * 1024 * 1024;

/// How much of the first user turn names the conversation.
pub const TITLE_MAX_CHARS: usize = 60;

/// What a conversation is called until a turn or the user names it.
const DEFAULT_TITLE: &str = "New chat";

/// One conversation, which is what a chat file holds.
///
/// It carries the turns, the proposals with their status, and attachments by
/// path, size and digest. It carries no API key and no attached note's text:
/// the note is on disk, and a second copy here would be a copy ADR-028 forbids
/// (ADR-031 rule 5.2, as amended by ADR-040).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Conversation {
    /// [`CONVERSATION_SCHEMA_VERSION`] at the time it was written.
    pub version: u32,
    /// The conversation's id, which is also its file name.
    pub id: String,
    /// What the pane calls it.
    pub title: String,
    /// When it was created, RFC 3339.
    pub created_at: String,
    /// When it last changed, RFC 3339.
    pub updated_at: String,
    /// The provider the turns were sent to.
    pub provider: String,
    /// The model the turns were sent to.
    pub model: String,
    /// The turns, oldest first.
    pub turns: Vec<StoredTurn>,
}

/// One turn as the file holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredTurn {
    /// Who said it.
    pub role: Role,
    /// What was said, with any proposal already removed (section 9).
    pub content: String,
    /// The notes a user turn put in front of the model.
    #[serde(default)]
    pub attachments: Vec<AttachmentRef>,
    /// The changes an assistant turn asked for.
    #[serde(default)]
    pub proposals: Vec<StoredProposal>,
    /// Which connection answered, for an assistant turn. Absent in files
    /// written before the field existed, and on every user turn.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity: Option<RequestIdentity>,
    /// The blocks an assistant turn wrote that could not become a proposal.
    ///
    /// Defaulted on load, so a file written before this existed reads as a
    /// turn that dropped nothing.
    #[serde(default)]
    pub dropped: Vec<DroppedProposal>,
    /// The reply reached the model's token ceiling, so it is the start of an
    /// answer rather than the whole of one.
    #[serde(default)]
    pub truncated: bool,
}

/// An attached note, named rather than copied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentRef {
    /// The note's path, folder-relative.
    pub path: String,
    /// How large the note was when it was attached.
    pub bytes: u64,
    /// The digest of what was read, hex-encoded.
    pub hash: String,
}

/// What a finished reply produced, beside its text.
///
/// Held together because a turn is written once: the proposals a person can
/// apply, the blocks that could not become one, and whether the reply reached
/// the token ceiling before it finished.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AssistantReply {
    /// The offers the card shows.
    pub proposals: Vec<StoredProposal>,
    /// The blocks that were dropped, with the reason.
    pub dropped: Vec<DroppedProposal>,
    /// The reply was cut off at the token ceiling.
    pub truncated: bool,
}

/// A proposal as the file holds it.
///
/// The hunks of [`Proposal`] are not written: they are a view of two texts,
/// one of which is the note on disk, and a note read now may have moved on.
/// They are computed again when the conversation is opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredProposal {
    /// The note it changes.
    pub path: String,
    /// The one line the model gave for it, empty when it gave none.
    pub summary: String,
    /// What the note held when the request was built.
    pub before_hash: String,
    /// The whole text the note would hold.
    pub new_content: String,
    /// What became of it.
    pub status: ProposalStatus,
}

/// What became of one proposal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProposalStatus {
    /// Nobody has decided yet.
    Pending,
    /// The user wrote it to the note.
    Applied,
    /// The user turned it down.
    Discarded,
    /// Writ turned it down, because the note had changed since it was read.
    Refused,
}

impl Conversation {
    /// An empty conversation, created at `now`.
    pub fn new(id: String, now: String, provider: String, model: String) -> Self {
        Self {
            version: CONVERSATION_SCHEMA_VERSION,
            id,
            title: DEFAULT_TITLE.to_string(),
            created_at: now.clone(),
            updated_at: now,
            provider,
            model,
            turns: Vec::new(),
        }
    }

    /// The title one turn's text gives a conversation.
    ///
    /// The first line that holds something, trimmed and cut at
    /// [`TITLE_MAX_CHARS`] characters. Text that holds nothing keeps the
    /// default, so an untitled conversation reads as one rather than as a
    /// blank row.
    pub fn title_from(text: &str) -> String {
        text.lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .map(|line| line.chars().take(TITLE_MAX_CHARS).collect())
            .unwrap_or_else(|| DEFAULT_TITLE.to_string())
    }

    /// Appends a user turn, naming the conversation if it is still unnamed.
    pub fn push_user(&mut self, content: String, attachments: Vec<AttachmentRef>, now: String) {
        if self.title == DEFAULT_TITLE {
            self.title = Self::title_from(&content);
        }
        self.turns.push(StoredTurn {
            role: Role::User,
            content,
            attachments,
            proposals: Vec::new(),
            identity: None,
            dropped: Vec::new(),
            truncated: false,
        });
        self.updated_at = now;
    }

    /// Appends an assistant turn, what it carried and lost, and the
    /// connection that answered.
    pub fn push_assistant(
        &mut self,
        content: String,
        reply: AssistantReply,
        identity: Option<RequestIdentity>,
        now: String,
    ) {
        self.turns.push(StoredTurn {
            role: Role::Assistant,
            content,
            attachments: Vec::new(),
            proposals: reply.proposals,
            identity,
            dropped: reply.dropped,
            truncated: reply.truncated,
        });
        self.updated_at = now;
    }

    /// Cuts the conversation back to `len` turns.
    ///
    /// This is what retrying and editing a turn do before the new turn is
    /// appended: what was after it is gone from the file on the next save.
    pub fn truncate(&mut self, len: usize, now: String) {
        self.turns.truncate(len);
        self.updated_at = now;
    }

    /// The turns as a request carries them: who spoke and what they said.
    ///
    /// Attachments and proposals stay here. A note reaches the model through
    /// the context the request is built with, read at send time, never from
    /// what a file remembers.
    pub fn request_turns(&self) -> Vec<ChatTurn> {
        self.turns
            .iter()
            .map(|turn| ChatTurn {
                role: turn.role,
                content: turn.content.clone(),
            })
            .collect()
    }

    /// Records what became of one proposal, returning false when the turn or
    /// the path names nothing.
    pub fn set_proposal_status(
        &mut self,
        turn: usize,
        path: &str,
        status: ProposalStatus,
        now: String,
    ) -> bool {
        let Some(turn) = self.turns.get_mut(turn) else {
            return false;
        };
        let Some(proposal) = turn.proposals.iter_mut().find(|item| item.path == path) else {
            return false;
        };
        proposal.status = status;
        self.updated_at = now;
        true
    }
}

impl From<&Proposal> for StoredProposal {
    fn from(proposal: &Proposal) -> Self {
        Self {
            path: proposal.path.clone(),
            summary: proposal.summary.clone(),
            before_hash: proposal.before_hash.clone(),
            new_content: proposal.new_content.clone(),
            status: ProposalStatus::Pending,
        }
    }
}
