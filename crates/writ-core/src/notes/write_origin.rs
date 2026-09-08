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

#[cfg(test)]
mod tests {
    use super::WriteOrigin;

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
