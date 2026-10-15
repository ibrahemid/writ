//! Who asked for a write to a note's file.
//!
//! Every write goes through one guarded path (`writ_storage::guarded`), and
//! the path is reached by the editor, by the command line, by a tool a client
//! called and, later, by a proposal the user accepted. The record of what
//! happened to a note is only worth keeping if it says which of those it was,
//! so the origin travels with the write rather than being inferred at the far
//! end from a path or a timestamp.
//!
//! Pure: naming the origin is policy, performing the write is
//! `writ-storage`'s half.

use std::fmt;

use serde::{Deserialize, Serialize};

/// What asked for a write.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteOrigin {
    /// A save of the text in an open tab.
    Editor,
    /// A `writ` command the user ran in a terminal.
    Cli,
    /// A tool call from a connected client, named as the client named itself.
    Mcp {
        /// The client's own name, as it introduced itself.
        client: String,
    },
    /// Text the user accepted from a proposal.
    Chat,
    /// A past version of the note, put back.
    Restore,
    /// A link rewritten because the note it names was renamed.
    RenamePropagation,
    /// A save Writ made on its own while the tab sat idle.
    Autosave,
}

impl fmt::Display for WriteOrigin {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WriteOrigin::Editor => f.write_str("editor"),
            WriteOrigin::Cli => f.write_str("cli"),
            WriteOrigin::Mcp { client } => write!(f, "mcp:{client}"),
            WriteOrigin::Chat => f.write_str("chat"),
            WriteOrigin::Restore => f.write_str("restore"),
            WriteOrigin::RenamePropagation => f.write_str("rename_propagation"),
            WriteOrigin::Autosave => f.write_str("autosave"),
        }
    }
}

/// Who left a version's text in the file, as the version list names it.
///
/// Read back from the origin a version row keeps ([`WriteOrigin`]'s display
/// form). Every surface of Writ that writes on the person's own action is
/// `Writ`: a save, an autosave, a restore, a link rewritten after a rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Writer {
    /// Writ, on the person's own action.
    Writ,
    /// A connected program, by the name it sent.
    Program {
        /// The name the program sent, as the grants in the settings key it.
        name: String,
    },
    /// Text accepted from the chat pane.
    Chat,
    /// The `writ` command, run in a terminal.
    Command,
}

impl Writer {
    /// The writer a stored origin names, or `None` for a spelling no origin
    /// writes.
    pub fn from_stored(stored: &str) -> Option<Self> {
        match stored {
            "editor" | "autosave" | "restore" | "rename_propagation" => Some(Self::Writ),
            "chat" => Some(Self::Chat),
            "cli" => Some(Self::Command),
            other => other
                .strip_prefix("mcp:")
                .filter(|name| !name.trim().is_empty())
                .map(|name| Self::Program {
                    name: name.to_string(),
                }),
        }
    }

    /// Whether the text came from a program connected to Writ.
    pub fn is_program(&self) -> bool {
        matches!(self, Self::Program { .. })
    }
}

impl From<&WriteOrigin> for Writer {
    fn from(origin: &WriteOrigin) -> Self {
        match origin {
            WriteOrigin::Mcp { client } => Self::Program {
                name: client.clone(),
            },
            WriteOrigin::Chat => Self::Chat,
            WriteOrigin::Cli => Self::Command,
            WriteOrigin::Editor
            | WriteOrigin::Autosave
            | WriteOrigin::Restore
            | WriteOrigin::RenamePropagation => Self::Writ,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{WriteOrigin, Writer};

    #[test]
    fn every_stored_origin_reads_back_as_the_writer_it_names() {
        for origin in [
            WriteOrigin::Editor,
            WriteOrigin::Cli,
            WriteOrigin::Mcp {
                client: "claude-code".to_string(),
            },
            WriteOrigin::Chat,
            WriteOrigin::Restore,
            WriteOrigin::RenamePropagation,
            WriteOrigin::Autosave,
        ] {
            assert_eq!(
                Writer::from_stored(&origin.to_string()),
                Some(Writer::from(&origin)),
                "{origin}"
            );
        }
        assert_eq!(Writer::from_stored("mcp:"), None);
        assert_eq!(Writer::from_stored("mcp:  "), None);
        assert_eq!(Writer::from_stored("something else"), None);
        assert!(Writer::from_stored("mcp:codex").is_some_and(|w| w.is_program()));
        assert!(!Writer::Chat.is_program());
    }

    #[test]
    fn a_writer_is_sent_tagged_by_kind() {
        assert_eq!(
            serde_json::to_string(&Writer::Program {
                name: "codex".to_string()
            })
            .expect("serialize"),
            "{\"kind\":\"program\",\"name\":\"codex\"}"
        );
        assert_eq!(
            serde_json::to_string(&Writer::Writ).expect("serialize"),
            "{\"kind\":\"writ\"}"
        );
    }

    #[test]
    fn every_origin_displays_as_one_lowercase_word() {
        assert_eq!(WriteOrigin::Editor.to_string(), "editor");
        assert_eq!(WriteOrigin::Cli.to_string(), "cli");
        assert_eq!(WriteOrigin::Chat.to_string(), "chat");
        assert_eq!(WriteOrigin::Restore.to_string(), "restore");
        assert_eq!(
            WriteOrigin::RenamePropagation.to_string(),
            "rename_propagation"
        );
        assert_eq!(WriteOrigin::Autosave.to_string(), "autosave");
    }

    #[test]
    fn a_client_names_itself_in_the_origin_it_writes_under() {
        let origin = WriteOrigin::Mcp {
            client: "some editor".to_string(),
        };
        assert_eq!(origin.to_string(), "mcp:some editor");
    }

    #[test]
    fn an_origin_survives_a_round_trip_through_json() {
        for origin in [
            WriteOrigin::Editor,
            WriteOrigin::Cli,
            WriteOrigin::Mcp {
                client: "some editor".to_string(),
            },
            WriteOrigin::Chat,
            WriteOrigin::Restore,
            WriteOrigin::RenamePropagation,
            WriteOrigin::Autosave,
        ] {
            let json = serde_json::to_string(&origin).expect("serialize");
            let back: WriteOrigin = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(back, origin);
        }
    }

    #[test]
    fn the_wire_name_of_an_origin_is_snake_case() {
        assert_eq!(
            serde_json::to_string(&WriteOrigin::RenamePropagation).expect("serialize"),
            "\"rename_propagation\""
        );
        assert_eq!(
            serde_json::to_string(&WriteOrigin::Mcp {
                client: "some editor".to_string()
            })
            .expect("serialize"),
            "{\"mcp\":{\"client\":\"some editor\"}}"
        );
    }
}
