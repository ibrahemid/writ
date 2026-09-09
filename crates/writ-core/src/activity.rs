//! What Writ did on someone else's behalf, and what it decided about it.
//!
//! One record type covers every actor that reaches a note through the harness:
//! an MCP client, the chat pane, and the app itself. [`ActivityRecord`] carries
//! time, actor, action, path, decision and byte count and **no field capable of
//! holding note text**, so ADR-031 rule 5.1 is a property of the type rather
//! than something review has to catch. A better log line is not available to
//! write, because there is nowhere to put one.
//!
//! [`ClientId`] and [`Decision`] live here rather than in `writ-mcp` so the
//! name a client sent and the verdict a gate reached are one type across the
//! server, the log and the settings surface (ADR-031 rule 3.1).

use std::path::PathBuf;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

/// The `clientInfo` a program sent at initialize.
///
/// A label the program chose for itself, never an identity (ADR-031 rule 3.1).
/// Matched exactly: a name differing by case or by surrounding space is another
/// client and starts at [`Decision::Pending`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct ClientId {
    /// The name the program calls itself.
    pub name: String,
    /// Its version, when it sent one.
    pub version: Option<String>,
}

impl ClientId {
    /// A client identified by name alone.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            version: None,
        }
    }
}

/// What was decided about one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Decision {
    /// The call ran.
    Allow,
    /// The call was refused, and the user had already decided that.
    Refuse,
    /// The call was refused, and the user has not been asked yet.
    Pending,
}

/// A client the harness has seen and the user has not decided on.
///
/// Kept apart from the activity log on purpose: the log is capped and rotates,
/// so a program looping calls it is not allowed to make can push the rows the
/// user needs in order to decide about it off the end. This record is one entry
/// per name, so no volume of calls can evict it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingClient {
    /// The name the program sent.
    pub name: String,
    /// Its version, when it sent one. The newest one seen under this name.
    #[serde(default)]
    pub version: Option<String>,
    /// When a call from this name was first seen.
    pub first_seen: chrono::DateTime<chrono::Utc>,
    /// When one was last recorded here.
    pub last_seen: chrono::DateTime<chrono::Utc>,
    /// How many calls have been recorded under this name.
    ///
    /// The file is written at most once a minute per client, so a process that
    /// exits mid-minute takes its unwritten calls with it: this is a floor, not
    /// a meter. Nothing decides anything on it.
    #[serde(default)]
    pub calls: u64,
}

impl PendingClient {
    /// A client seen for the first time, at `now`.
    pub fn first_call(client: &ClientId, now: chrono::DateTime<chrono::Utc>) -> Self {
        Self {
            name: client.name.clone(),
            version: client.version.clone(),
            first_seen: now,
            last_seen: now,
            calls: 1,
        }
    }
}

/// Who the action was performed for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Actor {
    /// A program calling a tool over MCP.
    Client {
        /// The name it sent.
        name: String,
        /// The version it sent, when it sent one.
        version: Option<String>,
    },
    /// The chat pane, talking to one endpoint.
    Chat {
        /// The host the request went to.
        host: String,
    },
    /// Writ itself, on the user's own action.
    App,
}

impl From<&ClientId> for Actor {
    fn from(client: &ClientId) -> Self {
        Actor::Client {
            name: client.name.clone(),
            version: client.version.clone(),
        }
    }
}

/// One line of the activity log.
///
/// `action` is a tool or operation name, `path` is the note it touched, and
/// `bytes` is a length. Nothing here holds what the note said.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityRecord {
    /// When it happened, written as RFC 3339 in UTC.
    #[serde(with = "rfc3339")]
    pub at: SystemTime,
    /// Who it was done for.
    pub actor: Actor,
    /// The tool or operation name.
    pub action: String,
    /// The note it touched, when it touched one.
    pub path: Option<PathBuf>,
    /// What was decided about it.
    pub decision: Decision,
    /// How many bytes were read or written, when that is a number.
    pub bytes: Option<u64>,
}

impl ActivityRecord {
    /// A record stamped with the current time.
    pub fn now(actor: Actor, action: impl Into<String>, decision: Decision) -> Self {
        Self {
            at: SystemTime::now(),
            actor,
            action: action.into(),
            path: None,
            decision,
            bytes: None,
        }
    }

    /// The same record, naming the note it touched.
    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    /// The same record, carrying a length.
    pub fn with_bytes(mut self, bytes: u64) -> Self {
        self.bytes = Some(bytes);
        self
    }
}

/// RFC 3339 in UTC for a [`SystemTime`], so a log line reads as a date and the
/// timestamp matches the spelling `first_seen` already uses in `config.toml`.
mod rfc3339 {
    use std::time::SystemTime;

    use chrono::{DateTime, SecondsFormat, Utc};
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(at: &SystemTime, serializer: S) -> Result<S::Ok, S::Error> {
        let stamp = DateTime::<Utc>::from(*at);
        serializer.serialize_str(&stamp.to_rfc3339_opts(SecondsFormat::Millis, true))
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<SystemTime, D::Error> {
        let text = String::deserialize(deserializer)?;
        let stamp = DateTime::parse_from_rfc3339(&text).map_err(serde::de::Error::custom)?;
        Ok(SystemTime::from(stamp.with_timezone(&Utc)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> ActivityRecord {
        ActivityRecord::now(
            Actor::Client {
                name: "Claude Code".to_string(),
                version: Some("1.2.3".to_string()),
            },
            "read_note",
            Decision::Allow,
        )
        .with_path("Ideas/Tessera.md")
        .with_bytes(412)
    }

    fn keys(value: &serde_json::Value) -> Vec<String> {
        value
            .as_object()
            .expect("an object")
            .keys()
            .cloned()
            .collect()
    }

    #[test]
    fn a_record_carries_no_key_that_could_hold_note_text() {
        let json = serde_json::to_value(record()).expect("serialise");
        let mut found = keys(&json);
        found.sort();
        assert_eq!(
            found,
            ["action", "actor", "at", "bytes", "decision", "path"]
        );
    }

    #[test]
    fn every_actor_serialises_without_a_content_key() {
        let actors = [
            Actor::Client {
                name: "Claude Code".to_string(),
                version: None,
            },
            Actor::Chat {
                host: "api.groq.com".to_string(),
            },
            Actor::App,
        ];
        for actor in actors {
            let json = serde_json::to_value(ActivityRecord::now(
                actor.clone(),
                "write_note",
                Decision::Refuse,
            ))
            .expect("serialise");
            let text = serde_json::to_string(&json).expect("encode");
            for banned in ["content", "text", "body", "excerpt", "prompt", "response"] {
                assert!(!text.contains(banned), "{actor:?} leaked {banned}");
            }
        }
    }

    #[test]
    fn a_record_round_trips_through_json() {
        let original = record();
        let line = serde_json::to_string(&original).expect("serialise");
        let parsed: ActivityRecord = serde_json::from_str(&line).expect("parse");
        // Millisecond resolution is what the line carries, so compare on it.
        assert_eq!(parsed.actor, original.actor);
        assert_eq!(parsed.action, original.action);
        assert_eq!(parsed.path, original.path);
        assert_eq!(parsed.decision, original.decision);
        assert_eq!(parsed.bytes, original.bytes);
    }

    #[test]
    fn the_timestamp_is_written_as_an_rfc_3339_string() {
        let json = serde_json::to_value(record()).expect("serialise");
        let at = json["at"].as_str().expect("a string");
        assert!(at.ends_with('Z'), "{at}");
        assert!(chrono::DateTime::parse_from_rfc3339(at).is_ok(), "{at}");
    }

    #[test]
    fn a_decision_is_written_in_lower_case() {
        for (decision, expected) in [
            (Decision::Allow, "allow"),
            (Decision::Refuse, "refuse"),
            (Decision::Pending, "pending"),
        ] {
            let json = serde_json::to_value(decision).expect("serialise");
            assert_eq!(json.as_str(), Some(expected));
        }
    }

    #[test]
    fn an_actor_is_tagged_by_kind() {
        let json = serde_json::to_value(Actor::App).expect("serialise");
        assert_eq!(json["kind"].as_str(), Some("app"));
        let json = serde_json::to_value(Actor::Chat {
            host: "localhost".to_string(),
        })
        .expect("serialise");
        assert_eq!(json["kind"].as_str(), Some("chat"));
    }

    #[test]
    fn an_actor_is_built_from_the_client_the_server_saw() {
        let client = ClientId {
            name: "Claude Code".to_string(),
            version: Some("1.2.3".to_string()),
        };
        assert_eq!(
            Actor::from(&client),
            Actor::Client {
                name: "Claude Code".to_string(),
                version: Some("1.2.3".to_string()),
            }
        );
    }

    #[test]
    fn a_client_named_alone_carries_no_version() {
        let client = ClientId::named("Some Client");
        assert_eq!(client.name, "Some Client");
        assert_eq!(client.version, None);
    }

    #[test]
    fn a_bare_record_names_no_path_and_no_length() {
        let bare = ActivityRecord::now(Actor::App, "save", Decision::Allow);
        assert_eq!(bare.path, None);
        assert_eq!(bare.bytes, None);
    }
}
