//! Who may call a tool, and in which direction.
//!
//! A client's name is a label it chose for itself and never an identity
//! (ADR-031 rule 3.1), so the gate decides on what the user approved, not on
//! what the client claims to be. [`ClientId`] and [`Decision`] are
//! `writ_core::activity`'s, re-exported here: the name the server saw, the
//! verdict it reached and the line the log keeps are one set of types.
//!
//! [`ConfigGate`] is the gate the server runs on. [`DenyAll`] is the default a
//! host takes when nothing was supplied, and [`EnabledReads`] is a fixture the
//! protocol tests drive; nothing in production constructs either.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use writ_core::activity::{ActivityRecord, Actor};
use writ_core::config::mcp::McpConfig;

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

/// The file the approvals are read from, inside the data folder.
const CONFIG_FILE: &str = "config.toml";

/// The `[mcp]` section, read on its own.
///
/// Only this section is deserialised, so a setting another release added, or a
/// section this build does not know, does not cost the gate its approvals.
#[derive(Debug, Default, Deserialize)]
struct ConfigFile {
    #[serde(default)]
    mcp: McpConfig,
}

/// What the last read of `config.toml` found, and what it was stamped with.
#[derive(Debug, Default)]
struct Cached {
    /// Modified time and length of the file as it was read. `None` is a file
    /// that was not there.
    stamp: Option<(SystemTime, u64)>,
    /// Whether a read has happened at all, which `stamp: None` alone cannot say.
    loaded: bool,
    /// The section as it was last read.
    config: McpConfig,
}

/// The gate the server runs on: `[mcp] approved_clients` in `config.toml`.
///
/// The list is re-read on every call, guarded by the file's modified time and
/// length, so approving a client in the app reaches a running server on its
/// next call and an unchanged file costs one `stat` (ADR-031 rule 3.5). The
/// gate never writes: approval is granted in the app, and no protocol message
/// can grant one (rule 3.4).
///
/// Every call it decides on is appended to the activity log, whichever way it
/// went (rule 5.5). The gate sees the client and the tool and not the path, so
/// the record names the tool; a consumer that knows which note it touched adds
/// that where it writes.
///
/// A client it has nobody's decision about is also written to
/// `mcp-pending.json`, once, however many calls it makes. The log is capped, so
/// a program looping calls it is refusing would otherwise push its own pending
/// rows off the end and leave the user nothing to decide on.
#[derive(Debug)]
pub struct ConfigGate {
    writ_dir: PathBuf,
    cached: Mutex<Cached>,
    parses: AtomicUsize,
    waiting: Mutex<HashMap<String, Waiting>>,
}

/// How often one client's entry in `mcp-pending.json` is rewritten. The first
/// call from a name is written at once; after that the file moves at most this
/// often, whatever rate the client calls at.
const PENDING_WRITE_EVERY: Duration = Duration::from_secs(60);

/// What this process knows about one waiting client since it started.
#[derive(Debug)]
struct Waiting {
    /// When its entry was last written.
    written: Instant,
    /// Calls counted since that write. Carried into the next one, so a client
    /// calling faster than the file is written still has every call counted
    /// unless the process exits first.
    unwritten: u64,
}

impl ConfigGate {
    /// A gate over the `config.toml` and activity log in `writ_dir`.
    pub fn new(writ_dir: impl Into<PathBuf>) -> Self {
        Self {
            writ_dir: writ_dir.into(),
            cached: Mutex::new(Cached::default()),
            parses: AtomicUsize::new(0),
            waiting: Mutex::new(HashMap::new()),
        }
    }

    /// How many times the file has actually been read.
    ///
    /// The mtime guard is the reason a long-lived server does not re-parse a
    /// file nobody edited, so it is observable rather than taken on trust.
    pub fn parse_count(&self) -> usize {
        self.parses.load(Ordering::Relaxed)
    }

    /// The `[mcp]` section as it is on disk now.
    fn current(&self) -> McpConfig {
        let path = self.writ_dir.join(CONFIG_FILE);
        let stamp = stamp_of(&path);
        let mut cached = self
            .cached
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if cached.loaded && cached.stamp == stamp {
            return cached.config.clone();
        }
        self.parses.fetch_add(1, Ordering::Relaxed);
        let config = read_section(&path);
        cached.stamp = stamp;
        cached.loaded = true;
        cached.config = config.clone();
        config
    }

    /// Appends the decision, dropping a log that could not be written.
    ///
    /// A data folder that cannot be written to is not a reason to answer a call
    /// differently: the decision already stands, and the write is the record of
    /// it.
    fn record(&self, client: &ClientId, tool: &str, decision: Decision) {
        let record = ActivityRecord::now(Actor::from(client), tool, decision);
        let _ = writ_storage::activity_log::append(&self.writ_dir, &record);
        if decision == Decision::Pending {
            self.note_waiting(client);
        }
    }

    /// Adds the client to `mcp-pending.json`, at most once a minute.
    ///
    /// The first call from a name is written straight away, so the user sees it
    /// as soon as it happens. After that the counted calls accumulate here and
    /// go in with the next write.
    fn note_waiting(&self, client: &ClientId) {
        let now = Instant::now();
        let due = {
            let mut waiting = self
                .waiting
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match waiting.get_mut(&client.name) {
                Some(seen) => {
                    seen.unwritten += 1;
                    if now.duration_since(seen.written) < PENDING_WRITE_EVERY {
                        None
                    } else {
                        Some(std::mem::replace(&mut seen.unwritten, 0))
                    }
                }
                None => {
                    waiting.insert(
                        client.name.clone(),
                        Waiting {
                            written: now,
                            unwritten: 0,
                        },
                    );
                    Some(1)
                }
            }
        };

        let Some(calls) = due else { return };
        let _ = writ_storage::pending_clients::note_calls(&self.writ_dir, client, calls);
        let mut waiting = self
            .waiting
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(seen) = waiting.get_mut(&client.name) {
            seen.written = now;
        }
    }
}

impl ConsentGate for ConfigGate {
    fn decide(&self, client: &ClientId, tool: &str) -> Decision {
        let config = self.current();
        let decision = verdict(&config, client, tool);
        self.record(client, tool, decision);
        decision
    }
}

/// The decision `config` reaches about `client` calling `tool`.
///
/// Pure, so the table of cases is testable without a folder. `Pending` is only
/// ever reached with the server on and a name to show: turning the server on
/// approves nobody (ADR-031 rule 7.2), and a client that sent no name leaves
/// nothing for the user to approve or revoke later.
fn verdict(config: &McpConfig, client: &ClientId, tool: &str) -> Decision {
    if !config.enabled || client.name.trim().is_empty() {
        return Decision::Refuse;
    }
    let Some(approval) = config.approval_for(&client.name) else {
        return Decision::Pending;
    };
    let granted = if READ_TOOLS.contains(&tool) {
        approval.read
    } else {
        approval.write
    };
    if granted {
        Decision::Allow
    } else {
        Decision::Refuse
    }
}

/// Modified time and length, or `None` for a file that is not there.
fn stamp_of(path: &Path) -> Option<(SystemTime, u64)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

/// The `[mcp]` section of the file at `path`.
///
/// Absent, unreadable or unparseable reads as the default section, which
/// approves nobody and holds the server off. Nothing here turns it on by
/// guessing.
fn read_section(path: &Path) -> McpConfig {
    let Ok(text) = std::fs::read_to_string(path) else {
        return McpConfig::default();
    };
    toml::from_str::<ConfigFile>(&text)
        .map(|file| file.mcp)
        .unwrap_or_default()
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

    // --- ConfigGate ---------------------------------------------------------

    fn config_dir() -> tempfile::TempDir {
        tempfile::TempDir::new().expect("temp dir")
    }

    fn write_config(dir: &std::path::Path, body: &str) {
        std::fs::write(dir.join(CONFIG_FILE), body).expect("seed config");
    }

    fn approving(name: &str, read: bool, write: bool) -> String {
        format!(
            "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"{name}\"\nread = {read}\nwrite = {write}\n"
        )
    }

    fn records(dir: &std::path::Path) -> Vec<writ_core::activity::ActivityRecord> {
        writ_storage::activity_log::read_recent(dir, 100)
    }

    #[test]
    fn an_unknown_client_is_pending_and_leaves_one_record_to_decide_on() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Pending
        );

        let logged = records(dir.path());
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].decision, Decision::Pending);
        assert_eq!(logged[0].action, "read_note");
        assert_eq!(
            logged[0].actor,
            Actor::Client {
                name: "Claude Code".to_string(),
                version: None
            }
        );
    }

    #[test]
    fn each_call_from_an_unknown_client_leaves_exactly_one_record() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Claude Code"), "list_notes");

        assert_eq!(records(dir.path()).len(), 2);
    }

    /// The activity log is capped, so a program looping calls it is not allowed
    /// to make would otherwise push its own pending rows off the end. The file
    /// the panel reads holds one entry per name and does not grow with the
    /// calls.
    #[test]
    fn a_client_refused_ten_thousand_times_is_recorded_once() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        for _ in 0..10_000 {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), "read_note"),
                Decision::Pending
            );
        }

        let waiting = writ_storage::pending_clients::read(dir.path());
        assert_eq!(waiting.len(), 1);
        assert_eq!(waiting[0].name, "Claude Code");

        let size = std::fs::metadata(writ_storage::pending_clients::path(dir.path()))
            .expect("the file")
            .len();
        assert!(size < 4_096, "the file grew with the calls: {size} bytes");
    }

    #[test]
    fn a_decided_client_is_never_written_to_the_waiting_file() {
        let dir = config_dir();
        write_config(
            dir.path(),
            "[mcp]\nenabled = true\n[[mcp.approved_clients]]\nname = \"Claude Code\"\nread = true\n",
        );
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Claude Code"), "write_note");

        assert!(writ_storage::pending_clients::read(dir.path()).is_empty());
    }

    #[test]
    fn two_waiting_clients_are_two_entries() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Zed"), "read_note");

        let names: Vec<String> = writ_storage::pending_clients::read(dir.path())
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Claude Code".to_string()));
        assert!(names.contains(&"Zed".to_string()));
    }

    /// Approval is granted in the app and nowhere else (ADR-031 rule 3.4). What
    /// the server writes is the log and the file naming who is waiting; the
    /// settings it decides from it only ever reads.
    #[test]
    fn deciding_writes_the_log_and_the_waiting_file_and_nothing_else() {
        let dir = config_dir();
        let settings = "[mcp]\nenabled = true\n";
        write_config(dir.path(), settings);
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Claude Code"), "write_note");

        assert_eq!(
            std::fs::read_to_string(dir.path().join(CONFIG_FILE)).expect("settings"),
            settings,
            "the gate edited the settings it decides from"
        );

        let mut written: Vec<String> = std::fs::read_dir(dir.path())
            .expect("list")
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .collect();
        written.sort();
        assert_eq!(
            written,
            [
                "activity.jsonl",
                "activity.lock",
                "config.toml",
                "mcp-pending.json"
            ]
        );
    }

    #[test]
    fn a_server_that_is_off_refuses_without_leaving_a_client_to_approve() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Refuse
        );
        let logged = records(dir.path());
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].decision, Decision::Refuse);
    }

    #[test]
    fn a_folder_with_no_config_answers_no_client() {
        let dir = config_dir();
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Refuse
        );
    }

    #[test]
    fn a_config_that_is_not_toml_answers_no_client() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp\nenabled = true");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Refuse
        );
    }

    #[test]
    fn a_section_this_build_does_not_know_does_not_cost_the_approvals() {
        let dir = config_dir();
        write_config(
            dir.path(),
            "[something_later]\nfield = 3\n\n[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Claude Code\"\nread = true\nwrite = false\n",
        );
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Allow
        );
    }

    #[test]
    fn approving_reading_does_not_approve_writing() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Allow
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Decision::Refuse
        );
    }

    #[test]
    fn approving_writing_does_not_approve_reading() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", false, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Refuse
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Decision::Allow
        );
    }

    #[test]
    fn an_approval_written_between_two_calls_lands_on_the_second() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Pending
        );

        write_config(dir.path(), &approving("Claude Code", true, false));

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Allow,
            "the same live gate, with no restart"
        );
    }

    #[test]
    fn forgetting_a_client_returns_it_to_pending_on_the_next_call() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Allow
        );

        write_config(dir.path(), "[mcp]\nenabled = true\n");

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Decision::Pending
        );
    }

    #[test]
    fn revoking_one_direction_lands_on_the_next_call() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Decision::Allow
        );

        write_config(dir.path(), &approving("Claude Code", true, false));

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Decision::Refuse
        );
    }

    #[test]
    fn an_unchanged_config_is_read_once_however_many_calls_arrive() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());

        for _ in 0..5 {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), "read_note"),
                Decision::Allow
            );
        }

        assert_eq!(gate.parse_count(), 1);
    }

    #[test]
    fn an_edited_config_is_read_again() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        gate.decide(&ClientId::named("Claude Code"), "read_note");

        write_config(dir.path(), &approving("Claude Code", true, false));
        gate.decide(&ClientId::named("Claude Code"), "read_note");

        assert_eq!(gate.parse_count(), 2);
    }

    #[test]
    fn a_name_differing_by_case_or_space_is_not_the_approved_client() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());

        for name in ["claude code", "CLAUDE CODE", " Claude Code", "Claude Code "] {
            assert_eq!(
                gate.decide(&ClientId::named(name), "read_note"),
                Decision::Pending,
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_client_that_sent_no_name_is_refused_by_the_config_gate_too() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        for name in ["", "   "] {
            assert_eq!(
                gate.decide(&ClientId::named(name), "read_note"),
                Decision::Refuse,
                "{name:?}"
            );
        }
    }

    #[test]
    fn a_decision_reaches_the_log_whichever_way_it_went() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Claude Code"), "write_note");
        gate.decide(&ClientId::named("Another Client"), "read_note");

        let logged = records(dir.path());
        let decisions: Vec<Decision> = logged.iter().map(|r| r.decision).collect();
        assert_eq!(
            decisions,
            [Decision::Pending, Decision::Refuse, Decision::Allow],
            "newest first"
        );
    }

    #[test]
    fn a_record_the_gate_writes_holds_no_note_text() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());
        gate.decide(&ClientId::named("Claude Code"), "read_note");

        let line = std::fs::read_to_string(writ_storage::activity_log::current_path(dir.path()))
            .expect("read log");
        let value: serde_json::Value = serde_json::from_str(line.trim()).expect("parse");
        let mut keys: Vec<&str> = value
            .as_object()
            .expect("object")
            .keys()
            .map(|k| k.as_str())
            .collect();
        keys.sort();
        assert_eq!(keys, ["action", "actor", "at", "bytes", "decision", "path"]);
    }
}
