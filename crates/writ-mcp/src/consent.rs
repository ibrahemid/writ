//! Who may call a tool, and in which direction.
//!
//! A client's name is a label it chose for itself and never an identity
//! (ADR-031 rule 3.1), so the gate decides on what the user approved, not on
//! what the client claims to be. [`ClientId`] and [`Decision`] are
//! `writ_core::activity`'s, re-exported here: the name the server saw, the
//! verdict it reached and the line the log keeps are one set of types.

use crate::tools::READ_TOOLS;

pub use writ_core::activity::{ClientId, Decision};

/// Decides whether one client may run one tool.
pub trait ConsentGate: Send + Sync {
    /// The decision for `client` calling `tool`.
    fn decide(&self, client: &ClientId, tool: &str) -> Decision;
}

/// Refuses everything.
///
/// The default a host takes when nothing else was supplied, so a wiring mistake
/// costs a refusal rather than a read.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAll;

impl ConsentGate for DenyAll {
    fn decide(&self, _client: &ClientId, _tool: &str) -> Decision {
        Decision::Refuse
    }
}

/// Grants the read tools while `mcp.enabled` is true, and nothing else.
///
/// This is 0.5's read half on its own. Turning `mcp.enabled` on is the user's
/// one act until U5 records per-client approvals; a client that sent no name is
/// refused either way, because there is nothing for the user to approve or
/// revoke later.
#[derive(Debug, Clone, Copy)]
pub struct EnabledReads {
    enabled: bool,
}

impl EnabledReads {
    /// A gate over the `mcp.enabled` setting.
    pub fn new(enabled: bool) -> Self {
        Self { enabled }
    }
}

impl ConsentGate for EnabledReads {
    fn decide(&self, client: &ClientId, tool: &str) -> Decision {
        if client.name.trim().is_empty() {
            return Decision::Refuse;
        }
        if !self.enabled {
            return Decision::Refuse;
        }
        if READ_TOOLS.contains(&tool) {
            Decision::Allow
        } else {
            Decision::Refuse
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deny_all_refuses_a_read_tool() {
        let gate = DenyAll;
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Refuse
        );
    }

    #[test]
    fn a_disabled_server_refuses_every_read_tool() {
        let gate = EnabledReads::new(false);
        for tool in READ_TOOLS {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), tool),
                Decision::Refuse,
                "{tool}"
            );
        }
    }

    #[test]
    fn an_enabled_server_allows_every_read_tool() {
        let gate = EnabledReads::new(true);
        for tool in READ_TOOLS {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), tool),
                Decision::Allow,
                "{tool}"
            );
        }
    }

    #[test]
    fn an_enabled_server_refuses_a_write_tool() {
        let gate = EnabledReads::new(true);
        for tool in ["write_note", "create_note", "rename_note", "trash_note"] {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), tool),
                Decision::Refuse,
                "{tool}"
            );
        }
    }

    #[test]
    fn a_client_that_sent_no_name_is_refused_even_when_the_server_is_on() {
        let gate = EnabledReads::new(true);
        for name in ["", "   "] {
            assert_eq!(
                gate.decide(&ClientId::named(name), "read_note"),
                Decision::Refuse,
                "{name:?}"
            );
        }
    }
}
