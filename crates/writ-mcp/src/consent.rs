//! Who may call a tool, and in which direction.
//!
//! A client's name is a label it chose for itself and never an identity
//! (ADR-031 rule 3.1), so the gate decides on what the user approved, not on
//! what the client claims to be. [`ClientId`] and [`Decision`] are
//! `writ_core::activity`'s, re-exported here: the name the server saw and the
//! line the log keeps are one set of types. The gate answers with a
//! [`Verdict`], which says why as well as what: the log keeps the three
//! answers a [`Decision`] holds, and the client is told the reason, because
//! each reason has a different fix.
//!
//! [`ConfigGate`] is the gate the server runs on. [`DenyAll`] is the default a
//! host takes when nothing was supplied, and [`EnabledReads`] is a fixture the
//! protocol tests drive; nothing in production constructs either.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

use serde::Deserialize;
use writ_core::config::mcp::McpConfig;

use crate::tools::{CONTEXT_TOOLS, READ_TOOLS};

pub use writ_core::activity::{ClientId, Decision};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Allow,
    Off,
    Unnamed,
    Waiting,
    NotGranted,
}

impl Verdict {
    pub fn decision(self) -> Decision {
        match self {
            Verdict::Allow => Decision::Allow,
            Verdict::Waiting => Decision::Pending,
            Verdict::Off | Verdict::Unnamed | Verdict::NotGranted => Decision::Refuse,
        }
    }
}

/// Decides whether one client may run one tool.
pub trait ConsentGate: Send + Sync {
    /// The verdict on `client` calling `tool`.
    fn decide(&self, client: &ClientId, tool: &str) -> Verdict;

    /// Whether each of `client`'s writes waits for the person to apply or
    /// discard it in Writ (ADR-053). A gate that keeps no such setting asks
    /// nobody.
    fn reviews_writes(&self, _client: &ClientId) -> bool {
        false
    }
}

/// Refuses everything, the way a server that is off does.
///
/// The default a host takes when nothing else was supplied, so a wiring mistake
/// costs a refusal rather than a read.
#[derive(Debug, Clone, Copy, Default)]
pub struct DenyAll;

impl ConsentGate for DenyAll {
    fn decide(&self, _client: &ClientId, _tool: &str) -> Verdict {
        Verdict::Off
    }
}

/// Grants the read tools while `mcp.enabled` is true, and nothing else: no
/// write tool and no tool that sees the open file.
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
    fn decide(&self, client: &ClientId, tool: &str) -> Verdict {
        if !self.enabled {
            return Verdict::Off;
        }
        if client.name.trim().is_empty() {
            return Verdict::Unnamed;
        }
        if READ_TOOLS.contains(&tool) {
            Verdict::Allow
        } else {
            Verdict::NotGranted
        }
    }
}

/// The file the approvals are read from, inside the data folder.
pub(crate) const CONFIG_FILE: &str = "config.toml";

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
/// gate never writes the settings: approval is granted in the app, and no
/// protocol message can grant one (rule 3.4).
///
/// The gate writes no line of the activity log. The tool host records each
/// call once, after the gate has answered, with the file the gate never sees
/// (rule 5.5).
///
/// A client it has nobody's decision about is written to `mcp-pending.json`
/// whenever that file does not list it, and while the file lists it, its entry
/// is rewritten at most once a minute however many calls it makes. That file
/// is the waiting list the app watches and shows. The app drops the entry when
/// the person decides. Not allowing a program is a decision too, so it is not
/// written there again until the person forgets it.
///
/// While the server is off, the first named client to call in this process is
/// written to `mcp-off-call.json`, its name and the time and nothing else, so
/// the app can offer to turn Connected programs on (ADR-031 rule 7.2).
#[derive(Debug)]
pub struct ConfigGate {
    writ_dir: PathBuf,
    cached: Mutex<Cached>,
    parses: AtomicUsize,
    waiting: Mutex<HashMap<String, Waiting>>,
    listing: Mutex<Option<Listing>>,
    off_noted: AtomicBool,
}

/// How often one client's entry in `mcp-pending.json` is rewritten while the
/// file lists it, whatever rate the client calls at.
const PENDING_WRITE_EVERY: Duration = Duration::from_secs(60);

/// What this process knows about one waiting client since it started.
#[derive(Debug, Default)]
struct Waiting {
    /// When its entry was last written, `None` before the first write.
    written: Option<Instant>,
    /// Calls counted since that write. Carried into the next one, so a client
    /// calling faster than the file is written still has every call counted
    /// unless the process exits first.
    unwritten: u64,
}

#[derive(Debug)]
struct Listing {
    stamp: Option<(SystemTime, u64)>,
    names: HashSet<String>,
}

impl ConfigGate {
    /// A gate over the `config.toml` and activity log in `writ_dir`.
    pub fn new(writ_dir: impl Into<PathBuf>) -> Self {
        Self {
            writ_dir: writ_dir.into(),
            cached: Mutex::new(Cached::default()),
            parses: AtomicUsize::new(0),
            waiting: Mutex::new(HashMap::new()),
            listing: Mutex::new(None),
            off_noted: AtomicBool::new(false),
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

    /// Lists the client in `mcp-pending.json`.
    ///
    /// A data folder that cannot be written to is not a reason to answer a call
    /// differently: the verdict already stands.
    ///
    /// A call from a name the file does not list is written straight away, so
    /// the user sees it as soon as it happens. That includes a name this
    /// process wrote before: the app drops the entry when the person decides,
    /// and a Forget can follow before the program calls again. While the file
    /// lists the name, the counted calls accumulate here and go in with a
    /// write at most once a minute.
    fn note_waiting(&self, client: &ClientId) {
        let now = Instant::now();
        let recent = {
            let mut waiting = self
                .waiting
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let seen = waiting.entry(client.name.clone()).or_default();
            seen.unwritten += 1;
            seen.written
                .is_some_and(|at| now.duration_since(at) < PENDING_WRITE_EVERY)
        };
        if recent && self.lists(&client.name) {
            return;
        }

        let calls = {
            let mut waiting = self
                .waiting
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let seen = waiting.entry(client.name.clone()).or_default();
            seen.written = Some(now);
            std::mem::take(&mut seen.unwritten)
        };
        let _ = writ_storage::pending_clients::note_calls(&self.writ_dir, client, calls);
        *self
            .listing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }

    /// Whether `mcp-pending.json` lists `name` now.
    ///
    /// Guarded by the file's modified time and length, as the settings are, so
    /// a file nobody rewrote costs one `stat`. The stamp is taken before the
    /// read, so a write landing during the read is caught on the next call.
    fn lists(&self, name: &str) -> bool {
        let stamp = stamp_of(&writ_storage::pending_clients::path(&self.writ_dir));
        if stamp.is_none() {
            return false;
        }
        let mut listing = self
            .listing
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(seen) = listing.as_ref().filter(|seen| seen.stamp == stamp) {
            return seen.names.contains(name);
        }
        let names: HashSet<String> = writ_storage::pending_clients::read(&self.writ_dir)
            .into_iter()
            .map(|entry| entry.name)
            .collect();
        let listed = names.contains(name);
        *listing = Some(Listing { stamp, names });
        listed
    }
}

impl ConsentGate for ConfigGate {
    fn reviews_writes(&self, client: &ClientId) -> bool {
        let config = self.current();
        config.enabled
            && config
                .approval_for(&client.name)
                .is_some_and(|approval| approval.reviews_writes())
    }

    fn decide(&self, client: &ClientId, tool: &str) -> Verdict {
        let config = self.current();
        let verdict = verdict(&config, client, tool);
        if verdict == Verdict::Waiting {
            self.note_waiting(client);
        }
        if verdict == Verdict::Off
            && !client.name.trim().is_empty()
            && !self.off_noted.swap(true, Ordering::Relaxed)
        {
            let _ = writ_storage::off_calls::note(&self.writ_dir, client);
        }
        verdict
    }
}

/// The verdict `config` reaches about `client` calling `tool`.
///
/// Pure, so the table of cases is testable without a folder. A server that is
/// off answers that before anything else. [`Verdict::Waiting`] is only ever
/// reached with the server on and a name to show: turning the server on
/// approves nobody (ADR-031 rule 7.2), and a client that sent no name leaves
/// nothing for the user to approve or revoke later. Writing implies reading by
/// the time the section is loaded (`ClientApproval`), so the read check reads
/// one field. Seeing the open file is its own grant, which neither of the
/// others carries (ADR-053).
fn verdict(config: &McpConfig, client: &ClientId, tool: &str) -> Verdict {
    if !config.enabled {
        return Verdict::Off;
    }
    if client.name.trim().is_empty() {
        return Verdict::Unnamed;
    }
    let Some(approval) = config.approval_for(&client.name) else {
        return Verdict::Waiting;
    };
    let granted = if READ_TOOLS.contains(&tool) {
        approval.read
    } else if CONTEXT_TOOLS.contains(&tool) {
        approval.context
    } else {
        approval.write
    };
    if granted {
        Verdict::Allow
    } else {
        Verdict::NotGranted
    }
}

/// Modified time and length, or `None` for a file that is not there.
pub(crate) fn stamp_of(path: &Path) -> Option<(SystemTime, u64)> {
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
    fn deny_all_answers_as_a_server_that_is_off() {
        let gate = DenyAll;
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Off
        );
    }

    #[test]
    fn a_disabled_server_refuses_every_read_tool_as_off() {
        let gate = EnabledReads::new(false);
        for tool in READ_TOOLS {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), tool),
                Verdict::Off,
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
                Verdict::Allow,
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
                Verdict::NotGranted,
                "{tool}"
            );
        }
    }

    #[test]
    fn an_enabled_server_does_not_show_the_open_file() {
        let gate = EnabledReads::new(true);
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "get_context"),
            Verdict::NotGranted
        );
    }

    #[test]
    fn a_client_that_sent_no_name_is_refused_even_when_the_server_is_on() {
        let gate = EnabledReads::new(true);
        for name in ["", "   "] {
            assert_eq!(
                gate.decide(&ClientId::named(name), "read_note"),
                Verdict::Unnamed,
                "{name:?}"
            );
        }
    }

    #[test]
    fn the_log_keeps_three_answers_and_the_client_hears_five_reasons() {
        assert_eq!(Verdict::Allow.decision(), Decision::Allow);
        assert_eq!(Verdict::Waiting.decision(), Decision::Pending);
        for refused in [Verdict::Off, Verdict::Unnamed, Verdict::NotGranted] {
            assert_eq!(refused.decision(), Decision::Refuse, "{refused:?}");
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

    fn waiting_names(dir: &std::path::Path) -> Vec<String> {
        writ_storage::pending_clients::read(dir)
            .into_iter()
            .map(|entry| entry.name)
            .collect()
    }

    #[test]
    fn writes_are_held_only_for_a_program_that_may_write_and_was_set_to_ask_first() {
        let dir = config_dir();
        let gate = ConfigGate::new(dir.path());
        let claude = ClientId::named("Claude Code");

        write_config(
            dir.path(),
            &format!("{}review = true\n", approving("Claude Code", true, true)),
        );
        assert!(gate.reviews_writes(&claude));
        assert!(!gate.reviews_writes(&ClientId::named("Codex")));

        write_config(
            dir.path(),
            &format!("{}review = true\n", approving("Claude Code", true, false)),
        );
        assert!(
            !gate.reviews_writes(&claude),
            "a program that cannot write has nothing held"
        );

        write_config(dir.path(), &approving("Claude Code", true, true));
        assert!(!gate.reviews_writes(&claude));

        write_config(
            dir.path(),
            &format!("{}review = true\n", approving("Claude Code", true, true))
                .replace("enabled = true", "enabled = false"),
        );
        assert!(!gate.reviews_writes(&claude));
        assert!(!DenyAll.reviews_writes(&claude));
    }

    #[test]
    fn an_unknown_client_waits_and_is_put_on_the_waiting_list() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Waiting
        );

        assert_eq!(waiting_names(dir.path()), ["Claude Code"]);
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
                Verdict::Waiting
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
    fn a_program_the_person_did_not_allow_is_refused_and_not_listed_again() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", false, false));
        let gate = ConfigGate::new(dir.path());

        for tool in ["list_notes", "read_note", "write_note"] {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), tool),
                Verdict::NotGranted,
                "{tool}"
            );
        }
        assert!(writ_storage::pending_clients::read(dir.path()).is_empty());
    }

    #[test]
    fn two_waiting_clients_are_two_entries() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Zed"), "read_note");

        let names = waiting_names(dir.path());
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"Claude Code".to_string()));
        assert!(names.contains(&"Zed".to_string()));
    }

    /// Approval is granted in the app and nowhere else (ADR-031 rule 3.4). What
    /// the gate writes is the file naming who is waiting; the settings it
    /// decides from it only ever reads, and the log is the tool host's.
    #[test]
    fn deciding_writes_the_waiting_file_and_nothing_else() {
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
            ["activity.lock", "config.toml", "mcp-pending.json"]
        );
    }

    #[test]
    fn a_server_that_is_off_refuses_without_leaving_a_client_to_approve() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Off
        );
        assert!(writ_storage::pending_clients::read(dir.path()).is_empty());
        assert!(!writ_storage::pending_clients::path(dir.path()).exists());
    }

    #[test]
    fn a_server_that_is_off_keeps_the_name_of_the_program_that_called() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");

        let call = writ_storage::off_calls::read(dir.path()).expect("a record");
        assert_eq!(call.name, "Claude Code");
        assert!(writ_storage::pending_clients::read(dir.path()).is_empty());
    }

    #[test]
    fn a_server_that_is_off_writes_its_record_once_per_process() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");
        gate.decide(&ClientId::named("Cursor"), "list_notes");

        let call = writ_storage::off_calls::read(dir.path()).expect("a record");
        assert_eq!(call.name, "Claude Code");
    }

    #[test]
    fn a_program_that_sent_no_name_leaves_no_record_while_off() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("  "), "read_note");

        assert!(!writ_storage::off_calls::path(dir.path()).exists());
    }

    #[test]
    fn a_server_that_is_on_leaves_no_off_record() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        gate.decide(&ClientId::named("Claude Code"), "read_note");

        assert!(!writ_storage::off_calls::path(dir.path()).exists());
    }

    #[test]
    fn a_server_that_is_off_says_so_before_anything_else() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = false\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(gate.decide(&ClientId::named(""), "read_note"), Verdict::Off);
    }

    #[test]
    fn a_folder_with_no_config_answers_no_client() {
        let dir = config_dir();
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Off
        );
    }

    #[test]
    fn a_config_that_is_not_toml_answers_no_client() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp\nenabled = true");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Off
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
            Verdict::Allow
        );
    }

    #[test]
    fn approving_reading_does_not_approve_writing() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, false));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Allow
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Verdict::NotGranted
        );
    }

    #[test]
    fn only_the_open_file_grant_lets_a_program_see_the_open_file() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, true));
        let gate = ConfigGate::new(dir.path());
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "get_context"),
            Verdict::NotGranted,
            "reading and writing do not carry it"
        );

        write_config(
            dir.path(),
            "[mcp]\nenabled = true\n\n[[mcp.approved_clients]]\nname = \"Claude Code\"\ncontext = true\n",
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "get_context"),
            Verdict::Allow
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::NotGranted,
            "and it carries neither"
        );
    }

    #[test]
    fn a_program_nobody_decided_on_waits_to_see_the_open_file() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        assert_eq!(
            gate.decide(&ClientId::named("Codex"), "get_context"),
            Verdict::Waiting
        );
        assert_eq!(waiting_names(dir.path()), ["Codex"]);
    }

    #[test]
    fn approving_writing_approves_reading_too() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", false, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Allow
        );
        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Verdict::Allow
        );
    }

    #[test]
    fn an_approval_written_between_two_calls_lands_on_the_second() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Waiting
        );

        write_config(dir.path(), &approving("Claude Code", true, false));

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Allow,
            "the same live gate, with no restart"
        );
    }

    #[test]
    fn forgetting_a_client_returns_it_to_waiting_on_the_next_call() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Allow
        );

        write_config(dir.path(), "[mcp]\nenabled = true\n");

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "read_note"),
            Verdict::Waiting
        );
    }

    #[test]
    fn a_program_approved_and_then_forgotten_is_listed_again_on_its_next_call() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        let program = ClientId::named("Claude Code");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        write_config(dir.path(), &approving("Claude Code", true, false));
        writ_storage::pending_clients::forget(dir.path(), "Claude Code", |_| {})
            .expect("the app drops the entry");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Allow);

        write_config(dir.path(), "[mcp]\nenabled = true\n");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        assert_eq!(waiting_names(dir.path()), ["Claude Code"]);
    }

    #[test]
    fn a_decision_undone_before_the_program_calls_again_lists_it_on_that_call() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        let program = ClientId::named("Claude Code");
        for _ in 0..3 {
            assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);
        }

        write_config(dir.path(), &approving("Claude Code", true, false));
        writ_storage::pending_clients::forget(dir.path(), "Claude Code", |_| {})
            .expect("the app drops the entry");
        write_config(dir.path(), "[mcp]\nenabled = true\n");

        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);
        assert_eq!(waiting_names(dir.path()), ["Claude Code"]);
    }

    #[test]
    fn a_program_not_allowed_and_then_forgotten_is_listed_again_on_its_next_call() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        let program = ClientId::named("Claude Code");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        write_config(dir.path(), &approving("Claude Code", false, false));
        writ_storage::pending_clients::forget(dir.path(), "Claude Code", |_| {})
            .expect("the app drops the entry");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::NotGranted);

        write_config(dir.path(), "[mcp]\nenabled = true\n");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        assert_eq!(waiting_names(dir.path()), ["Claude Code"]);
    }

    #[test]
    fn a_waiting_file_taken_away_is_written_again_on_the_next_call() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());
        let program = ClientId::named("Claude Code");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        std::fs::remove_file(writ_storage::pending_clients::path(dir.path()))
            .expect("remove the file");
        assert_eq!(gate.decide(&program, "read_note"), Verdict::Waiting);

        assert_eq!(waiting_names(dir.path()), ["Claude Code"]);
    }

    #[test]
    fn a_program_the_file_still_lists_is_not_written_again_within_the_minute() {
        let dir = config_dir();
        write_config(dir.path(), "[mcp]\nenabled = true\n");
        let gate = ConfigGate::new(dir.path());

        for _ in 0..5 {
            assert_eq!(
                gate.decide(&ClientId::named("Claude Code"), "read_note"),
                Verdict::Waiting
            );
        }

        let waiting = writ_storage::pending_clients::read(dir.path());
        assert_eq!(waiting.len(), 1);
        assert_eq!(
            waiting[0].calls, 1,
            "a later call was written within the minute"
        );
    }

    #[test]
    fn revoking_one_direction_lands_on_the_next_call() {
        let dir = config_dir();
        write_config(dir.path(), &approving("Claude Code", true, true));
        let gate = ConfigGate::new(dir.path());

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Verdict::Allow
        );

        write_config(dir.path(), &approving("Claude Code", true, false));

        assert_eq!(
            gate.decide(&ClientId::named("Claude Code"), "write_note"),
            Verdict::NotGranted
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
                Verdict::Allow
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
                Verdict::Waiting,
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
                Verdict::Unnamed,
                "{name:?}"
            );
        }
        assert!(writ_storage::pending_clients::read(dir.path()).is_empty());
    }
}
