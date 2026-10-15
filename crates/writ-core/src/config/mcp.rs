//! `[mcp]` configuration section.
//!
//! The server other programs speak to over stdio, off until the user turns it
//! on, and the list of clients they approved. Turning `enabled` on approves
//! nobody: a client not in `approved_clients` is refused and shown to the user
//! to decide on (ADR-031 rules 3.2 and 7.2). Reading and writing are separate
//! grants, so approving a client to read never lets it write. Seeing the file
//! open in Writ is a third, which neither of the others carries (ADR-053).
//!
//! The list is plain text the user can read and edit, which is the other half
//! of revocation (ADR-031 rule 6.1). Every field has a serde default so an
//! existing `config.toml` upgrades without an edit.

use serde::{Deserialize, Serialize};

fn default_enabled() -> bool {
    false
}

fn default_approved_clients() -> Vec<ClientApproval> {
    Vec::new()
}

fn default_first_seen() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::<chrono::Utc>::UNIX_EPOCH
}

/// What one client may do.
///
/// `name` is the `clientInfo` name the program sent, which is a label it chose
/// for itself and never an identity (ADR-031 rule 3.1). It is matched exactly:
/// a name differing by case or by surrounding space is a different client and
/// starts at pending.
///
/// Read through [`StoredApproval`], so an entry granting writing loads as
/// granting reading too, in the app and in the server alike: both parse this
/// type, whichever section they read it from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "StoredApproval")]
pub struct ClientApproval {
    /// The name the client sent.
    pub name: String,
    /// When Writ first saw a call from this name.
    pub first_seen: chrono::DateTime<chrono::Utc>,
    /// Whether it may run the read tools.
    pub read: bool,
    /// Whether it may run the write tools.
    pub write: bool,
    /// Whether it may see the file open in Writ and its selection.
    pub context: bool,
    /// Whether each of its writes waits for the person to apply or discard
    /// it in Writ (ADR-053). Read only while it may write.
    pub review: bool,
}

impl ClientApproval {
    /// Whether its writes are held for the person's answer: it may write, and
    /// the person asked to see each write first.
    pub fn reviews_writes(&self) -> bool {
        self.write && self.review
    }

    /// Whether the entry grants nothing: the person decided the program may
    /// neither read nor write nor see the open file.
    pub fn grants_nothing(&self) -> bool {
        !self.read && !self.write && !self.context
    }
}

#[derive(Debug, Deserialize)]
struct StoredApproval {
    name: String,
    #[serde(default = "default_first_seen")]
    first_seen: chrono::DateTime<chrono::Utc>,
    #[serde(default)]
    read: bool,
    #[serde(default)]
    write: bool,
    #[serde(default)]
    context: bool,
    #[serde(default)]
    review: bool,
}

impl From<StoredApproval> for ClientApproval {
    fn from(stored: StoredApproval) -> Self {
        Self {
            name: stored.name,
            first_seen: stored.first_seen,
            read: stored.read || stored.write,
            write: stored.write,
            context: stored.context,
            review: stored.review,
        }
    }
}

/// Configuration for the MCP server (`[mcp]`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpConfig {
    /// Master switch. When `false` (default) every tool call is refused.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// The clients the user approved, and in which direction. Written by the
    /// app; the command line never adds one (ADR-031 rule 3.4).
    #[serde(default = "default_approved_clients")]
    pub approved_clients: Vec<ClientApproval>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            enabled: default_enabled(),
            approved_clients: default_approved_clients(),
        }
    }
}

impl McpConfig {
    /// The approval recorded for `name`, matched exactly.
    pub fn approval_for(&self, name: &str) -> Option<&ClientApproval> {
        self.approved_clients
            .iter()
            .find(|approval| approval.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::WritConfig;

    #[test]
    fn a_fresh_configuration_answers_no_client() {
        let config = McpConfig::default();
        assert!(!config.enabled);
        assert!(config.approved_clients.is_empty());
    }

    #[test]
    fn a_config_without_the_section_loads_with_the_defaults() {
        let config: WritConfig = toml::from_str("[ai]\nenabled = true\n").expect("parse");
        assert_eq!(config.mcp, McpConfig::default());
    }

    #[test]
    fn a_section_without_approved_clients_loads() {
        let config: WritConfig = toml::from_str("[mcp]\nenabled = true\n").expect("parse");
        assert!(config.mcp.enabled);
        assert!(config.mcp.approved_clients.is_empty());
    }

    #[test]
    fn an_approval_round_trips_through_toml() {
        let approval = ClientApproval {
            name: "Claude Code".to_string(),
            first_seen: chrono::DateTime::parse_from_rfc3339("2026-09-09T10:30:00Z")
                .expect("timestamp")
                .with_timezone(&chrono::Utc),
            read: true,
            write: false,
            context: true,
            review: false,
        };
        let config = WritConfig {
            mcp: McpConfig {
                enabled: true,
                approved_clients: vec![approval.clone()],
            },
            ..WritConfig::default()
        };

        let text = toml::to_string(&config).expect("serialise");
        let parsed: WritConfig = toml::from_str(&text).expect("parse");

        assert!(parsed.mcp.enabled);
        assert_eq!(parsed.mcp.approved_clients, vec![approval]);
    }

    #[test]
    fn an_approval_without_a_direction_grants_neither() {
        let config: WritConfig =
            toml::from_str("[mcp]\n[[mcp.approved_clients]]\nname = \"Some Client\"\n")
                .expect("parse");
        let approval = config.mcp.approval_for("Some Client").expect("the client");
        assert!(!approval.read);
        assert!(!approval.write);
        assert!(!approval.context);
        assert_eq!(approval.first_seen, default_first_seen());
    }

    #[test]
    fn neither_reading_nor_writing_lets_a_program_see_the_open_file() {
        let config: WritConfig = toml::from_str(
            "[mcp]\n[[mcp.approved_clients]]\nname = \"Claude Code\"\nread = true\nwrite = true\n",
        )
        .expect("parse");
        let approval = config.mcp.approval_for("Claude Code").expect("the client");
        assert!(approval.write);
        assert!(!approval.context);

        let config: WritConfig =
            toml::from_str("[mcp]\n[[mcp.approved_clients]]\nname = \"Codex\"\ncontext = true\n")
                .expect("parse");
        let approval = config.mcp.approval_for("Codex").expect("the client");
        assert!(approval.context);
        assert!(!approval.read && !approval.write);
    }

    #[test]
    fn an_entry_granting_neither_direction_loads_as_a_decision_that_grants_nothing() {
        let config: WritConfig = toml::from_str(
            "[mcp]\n[[mcp.approved_clients]]\nname = \"Zed\"\nread = false\nwrite = false\n",
        )
        .expect("parse");
        let approval = config.mcp.approval_for("Zed").expect("the client");
        assert!(approval.grants_nothing());

        let reading = ClientApproval {
            name: "Zed".to_string(),
            first_seen: default_first_seen(),
            read: true,
            write: false,
            context: false,
            review: false,
        };
        assert!(!reading.grants_nothing());

        let seeing = ClientApproval {
            read: false,
            context: true,
            ..reading
        };
        assert!(!seeing.grants_nothing());
    }

    #[test]
    fn a_normalised_entry_round_trips_as_it_loaded() {
        let config: WritConfig = toml::from_str(
            "[mcp]\n[[mcp.approved_clients]]\nname = \"Claude Code\"\nwrite = true\n",
        )
        .expect("parse");
        let text = toml::to_string(&config).expect("serialise");
        let again: WritConfig = toml::from_str(&text).expect("parse again");
        assert_eq!(again.mcp, config.mcp);
        let approval = again.mcp.approval_for("Claude Code").expect("the client");
        assert!(approval.read && approval.write);
    }

    #[test]
    fn writing_implies_reading_once_the_settings_are_loaded() {
        let text = "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Claude Code\"\nread = false\nwrite = true\n";

        let config: WritConfig = toml::from_str(text).expect("parse");
        let approval = config.mcp.approval_for("Claude Code").expect("the client");
        assert!(approval.read, "the whole settings file");
        assert!(approval.write);

        #[derive(Deserialize)]
        struct SectionOnly {
            mcp: McpConfig,
        }
        let section: SectionOnly = toml::from_str(text).expect("parse");
        let approval = section.mcp.approval_for("Claude Code").expect("the client");
        assert!(approval.read, "the section on its own");
        assert!(approval.write);
    }

    #[test]
    fn a_name_differing_by_case_or_space_is_another_client() {
        let config = McpConfig {
            enabled: true,
            approved_clients: vec![ClientApproval {
                name: "Claude Code".to_string(),
                first_seen: default_first_seen(),
                read: true,
                write: false,
                context: false,
                review: false,
            }],
        };

        assert!(config.approval_for("Claude Code").is_some());
        assert!(config.approval_for("claude code").is_none());
        assert!(config.approval_for(" Claude Code").is_none());
    }
}
