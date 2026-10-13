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

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

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

/// The names people know some MCP clients by, keyed by the name each sends.
///
/// For display only: the gate, the stored approvals and the log keep the name
/// as sent (rule 3.1), so two names that read alike here are two clients.
/// `src/lib/program-names.ts` holds the same table for the app.
pub const PROGRAM_NAMES: &[(&str, &str)] = &[
    ("claude-code", "Claude Code"),
    ("claude-desktop", "Claude Desktop"),
    ("codex", "Codex"),
    ("codex-cli", "Codex"),
    ("cursor", "Cursor"),
    ("vscode", "VS Code"),
    ("code", "VS Code"),
    ("gemini-cli", "Gemini CLI"),
    ("lm-studio", "LM Studio"),
];

/// The name to show a person for a client that sent `sent`: its entry in
/// [`PROGRAM_NAMES`], or `sent` itself.
pub fn display_program_name(sent: &str) -> &str {
    PROGRAM_NAMES
        .iter()
        .find(|&&(name, _)| name == sent)
        .map_or(sent, |&(_, shown)| shown)
}

/// What was decided about one call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    /// How many calls the line stands for, when it is more than one: repeated
    /// reads within [`REPEAT_WINDOW`] share a line ([`RepeatFold`]). Absent
    /// from every other line, so a log written before it reads the same.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
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
            count: None,
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

    /// The same record, standing for `calls` calls. One call is an ordinary
    /// line and carries no count.
    pub fn with_count(mut self, calls: u64) -> Self {
        self.count = (calls > 1).then_some(calls);
        self
    }

    /// How many calls the line stands for.
    pub fn calls(&self) -> u64 {
        self.count.unwrap_or(1)
    }
}

/// How long repeated reads share one line of the log.
pub const REPEAT_WINDOW: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct FoldKey {
    actor: String,
    action: String,
    path: Option<PathBuf>,
    decision: Decision,
}

impl FoldKey {
    fn of(record: &ActivityRecord) -> Self {
        let actor = match &record.actor {
            Actor::Client { name, .. } => format!("client:{name}"),
            Actor::Chat { host } => format!("chat:{host}"),
            Actor::App => "app".to_string(),
        };
        Self {
            actor,
            action: record.action.clone(),
            path: record.path.clone(),
            decision: record.decision,
        }
    }
}

#[derive(Debug)]
struct Run {
    opened: Instant,
    held: u64,
    last: Option<ActivityRecord>,
}

/// Folds repeated reads into one line per [`REPEAT_WINDOW`] (ADR-031 rule
/// 5.5).
///
/// The first read of a run is written as it happens, so the log shows a
/// program reading a file at the moment it did. Further reads of the same file
/// by the same program, decided the same way, are counted until the window
/// closes; then one line carries the count, stamped at the last of them. A
/// window closes at the first record after it ends, whichever file that record
/// names, or when [`RepeatFold::drain`] is called as the log closes. A process
/// killed mid-window takes its held count with it, so a count is a floor.
///
/// Only the read tools fold. A write, an overwrite, a chat proposal or the
/// app's own action is always its own line.
#[derive(Debug)]
pub struct RepeatFold {
    window: Duration,
    runs: HashMap<FoldKey, Run>,
}

impl Default for RepeatFold {
    fn default() -> Self {
        Self::new(REPEAT_WINDOW)
    }
}

impl RepeatFold {
    /// A fold over runs `window` long.
    pub fn new(window: Duration) -> Self {
        Self {
            window,
            runs: HashMap::new(),
        }
    }

    /// The lines to append for `record`, arriving at `now`, in the order they
    /// go in: the counts of runs that have ended, oldest first, then `record`
    /// itself unless it joined a run.
    pub fn take(&mut self, record: ActivityRecord, now: Instant) -> Vec<ActivityRecord> {
        let mut due = self.close_ended(now);
        if !crate::tools::is_read_tool(&record.action) {
            due.push(record);
            return due;
        }
        match self.runs.get_mut(&FoldKey::of(&record)) {
            Some(run) => {
                run.held += 1;
                run.last = Some(record);
            }
            None => {
                self.runs.insert(
                    FoldKey::of(&record),
                    Run {
                        opened: now,
                        held: 0,
                        last: None,
                    },
                );
                due.push(record);
            }
        }
        due
    }

    /// Every count still held, as its line, oldest first. What a log that is
    /// closing writes last.
    pub fn drain(&mut self) -> Vec<ActivityRecord> {
        let mut lines: Vec<ActivityRecord> = self
            .runs
            .drain()
            .filter_map(|(_, run)| run.line())
            .collect();
        lines.sort_by_key(|line| line.at);
        lines
    }

    fn close_ended(&mut self, now: Instant) -> Vec<ActivityRecord> {
        let window = self.window;
        let ended: Vec<FoldKey> = self
            .runs
            .iter()
            .filter(|(_, run)| now.saturating_duration_since(run.opened) >= window)
            .map(|(key, _)| key.clone())
            .collect();
        let mut lines: Vec<ActivityRecord> = ended
            .into_iter()
            .filter_map(|key| self.runs.remove(&key))
            .filter_map(|run| run.line())
            .collect();
        lines.sort_by_key(|line| line.at);
        lines
    }
}

impl Run {
    fn line(self) -> Option<ActivityRecord> {
        let held = self.held;
        self.last
            .filter(|_| held > 0)
            .map(|last| last.with_count(held))
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

    #[test]
    fn a_known_client_reads_by_the_name_people_know_it_by() {
        assert_eq!(display_program_name("claude-code"), "Claude Code");
        assert_eq!(display_program_name("claude-desktop"), "Claude Desktop");
        assert_eq!(display_program_name("codex"), "Codex");
        assert_eq!(display_program_name("codex-cli"), "Codex");
        assert_eq!(display_program_name("cursor"), "Cursor");
        assert_eq!(display_program_name("vscode"), "VS Code");
        assert_eq!(display_program_name("code"), "VS Code");
        assert_eq!(display_program_name("gemini-cli"), "Gemini CLI");
        assert_eq!(display_program_name("lm-studio"), "LM Studio");
    }

    #[test]
    fn any_other_name_reads_exactly_as_it_was_sent() {
        for sent in [
            "Zed",
            "Claude-Code",
            " claude-code",
            "cursor ",
            "",
            "Claude Code",
        ] {
            assert_eq!(display_program_name(sent), sent);
        }
    }

    #[test]
    fn each_name_a_client_sends_has_one_entry() {
        let mut sent: Vec<&str> = PROGRAM_NAMES.iter().map(|&(name, _)| name).collect();
        let entries = sent.len();
        sent.sort_unstable();
        sent.dedup();
        assert_eq!(sent.len(), entries);
    }

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
    fn a_folded_line_carries_its_count_and_still_no_key_that_could_hold_text() {
        let json = serde_json::to_value(record().with_count(5)).expect("serialise");
        let mut found = keys(&json);
        found.sort();
        assert_eq!(
            found,
            ["action", "actor", "at", "bytes", "count", "decision", "path"]
        );
        assert_eq!(json["count"].as_u64(), Some(5));
    }

    #[test]
    fn one_call_is_an_ordinary_line_with_no_count() {
        let one = record().with_count(1);
        assert_eq!(one.count, None);
        assert_eq!(one.calls(), 1);
        let json = serde_json::to_value(one).expect("serialise");
        assert!(json.get("count").is_none());
    }

    #[test]
    fn a_line_written_before_the_count_existed_reads_as_one_call() {
        let line = r#"{"at":"2026-09-09T10:30:00.000Z","actor":{"kind":"app"},"action":"save","path":null,"decision":"allow","bytes":null}"#;
        let parsed: ActivityRecord = serde_json::from_str(line).expect("parse");
        assert_eq!(parsed.count, None);
        assert_eq!(parsed.calls(), 1);
    }

    fn read_at(path: &str, second: u64) -> ActivityRecord {
        let mut line = ActivityRecord::now(
            Actor::Client {
                name: "Claude Code".to_string(),
                version: None,
            },
            "read_note",
            Decision::Allow,
        )
        .with_path(path);
        line.at = SystemTime::UNIX_EPOCH + Duration::from_secs(second);
        line
    }

    fn write_at(second: u64) -> ActivityRecord {
        let mut line = ActivityRecord::now(
            Actor::Client {
                name: "Claude Code".to_string(),
                version: None,
            },
            "write_note",
            Decision::Allow,
        )
        .with_path("Launch.md")
        .with_bytes(6);
        line.at = SystemTime::UNIX_EPOCH + Duration::from_secs(second);
        line
    }

    #[test]
    fn the_first_read_of_a_run_is_written_as_it_happens() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();

        let due = fold.take(read_at("Launch.md", 0), start);

        assert_eq!(due.len(), 1);
        assert_eq!(due[0].count, None);
    }

    #[test]
    fn repeated_reads_in_the_window_become_one_line_with_their_count() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();
        fold.take(read_at("Launch.md", 0), start);

        for second in 1..=10 {
            let due = fold.take(
                read_at("Launch.md", second),
                start + Duration::from_secs(second),
            );
            assert!(due.is_empty(), "held at {second}s: {due:?}");
        }

        let due = fold.take(read_at("Launch.md", 61), start + Duration::from_secs(61));
        assert_eq!(due.len(), 2, "{due:?}");
        assert_eq!(due[0].count, Some(10), "the held calls, as one line");
        assert_eq!(
            due[0].at,
            SystemTime::UNIX_EPOCH + Duration::from_secs(10),
            "stamped at the last call it counts"
        );
        assert_eq!(due[1].count, None, "the call that opened the next run");
    }

    #[test]
    fn a_write_is_never_folded() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();

        for second in 0..5 {
            let due = fold.take(write_at(second), start + Duration::from_secs(second));
            assert_eq!(due.len(), 1, "write at {second}s");
            assert_eq!(due[0].count, None);
        }
    }

    #[test]
    fn another_file_tool_or_answer_is_another_run() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();
        fold.take(read_at("Launch.md", 0), start);

        let other_file = fold.take(read_at("Plan.md", 1), start + Duration::from_secs(1));
        let mut listing = read_at("Launch.md", 2);
        listing.action = "note_links".to_string();
        let other_tool = fold.take(listing, start + Duration::from_secs(2));
        let mut waiting = read_at("Launch.md", 3);
        waiting.decision = Decision::Pending;
        let other_answer = fold.take(waiting, start + Duration::from_secs(3));

        assert_eq!(other_file.len(), 1);
        assert_eq!(other_tool.len(), 1);
        assert_eq!(other_answer.len(), 1);
    }

    #[test]
    fn a_run_closes_at_the_first_record_after_its_window_whatever_it_names() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();
        fold.take(read_at("Launch.md", 0), start);
        fold.take(read_at("Launch.md", 5), start + Duration::from_secs(5));
        fold.take(read_at("Launch.md", 6), start + Duration::from_secs(6));

        let due = fold.take(write_at(70), start + Duration::from_secs(70));

        assert_eq!(due.len(), 2, "{due:?}");
        assert_eq!(due[0].action, "read_note");
        assert_eq!(due[0].count, Some(2));
        assert_eq!(due[1].action, "write_note");
    }

    #[test]
    fn a_closing_log_writes_every_count_it_holds() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();
        fold.take(read_at("Launch.md", 0), start);
        fold.take(read_at("Plan.md", 1), start + Duration::from_secs(1));
        fold.take(read_at("Launch.md", 2), start + Duration::from_secs(2));
        fold.take(read_at("Plan.md", 3), start + Duration::from_secs(3));
        fold.take(read_at("Plan.md", 4), start + Duration::from_secs(4));

        let drained = fold.drain();

        let summary: Vec<(Option<PathBuf>, u64)> = drained
            .iter()
            .map(|line| (line.path.clone(), line.calls()))
            .collect();
        assert_eq!(
            summary,
            [
                (Some(PathBuf::from("Launch.md")), 1),
                (Some(PathBuf::from("Plan.md")), 2)
            ],
            "oldest first"
        );
        assert!(fold.drain().is_empty(), "nothing is written twice");
    }

    #[test]
    fn a_run_with_nothing_held_writes_nothing_when_it_closes() {
        let mut fold = RepeatFold::default();
        let start = Instant::now();
        fold.take(read_at("Launch.md", 0), start);

        let due = fold.take(read_at("Launch.md", 90), start + Duration::from_secs(90));

        assert_eq!(due.len(), 1, "only the new run's first line: {due:?}");
        assert!(fold.drain().is_empty());
    }

    #[test]
    fn a_bare_record_names_no_path_and_no_length() {
        let bare = ActivityRecord::now(Actor::App, "save", Decision::Allow);
        assert_eq!(bare.path, None);
        assert_eq!(bare.bytes, None);
    }
}
