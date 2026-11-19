//! Moving the Windows login item to the new product name (ADR-059 decision 10).
//!
//! The login item is a value under the per-user `Run` key, named after the
//! product, plus an optional on/off state under `StartupApproved\Run`. Builds
//! from before the rename registered it as [`legacy_names::PRODUCT_NAME`];
//! this decides what moving it to the current name takes. The adapter reads
//! the registry and carries the plan out.

use crate::legacy_names;
use crate::startup::{is_login_command_line, GUI_BINARY_STEM};

/// The per-user key holding the commands Windows runs at sign-in.
pub const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// The per-user key holding whether each sign-in command is turned on.
pub const STARTUP_APPROVED_KEY: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

const EXE_EXTENSION: &str = ".exe";

/// What the registry holds, as the plan needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunValueFacts<'a> {
    /// The command under the old name, when there is one.
    pub legacy_command: Option<&'a str>,
    /// Whether a command is already registered under the current name.
    pub current_present: bool,
    /// The on/off state recorded for the old name, when there is one.
    pub legacy_approval: Option<&'a [u8]>,
    /// The product name this build registers its login item under.
    pub current_name: &'a str,
}

/// What moving the login item takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunValuePlan {
    /// Nothing to move: no login item under the old name, one that is not
    /// Jotra's, or a build that still runs under the old name.
    Untouched,
    /// A login item under the current name already exists; the old one goes so
    /// sign-in starts the app once and the app's own switch governs it.
    RemoveLegacy,
    /// Register `command` under the current name, copy `approval` beside it
    /// when there is one, then remove the old value and, when it was copied,
    /// the old state.
    Move {
        /// The old value's command, kept as it was: the launcher at the old
        /// path starts the current app.
        command: String,
        /// The old on/off state to copy.
        approval: Option<Vec<u8>>,
    },
}

/// The program a `Run` command starts: the quoted first word or, unquoted,
/// the shortest leading run of words ending in `.exe`, as Windows reads a
/// path with spaces written without quotes, else the first word.
fn run_command_program(command: &str) -> Option<&str> {
    let command = command.trim_start();
    if let Some(quoted) = command.strip_prefix('"') {
        let (program, _) = quoted.split_once('"')?;
        return (!program.is_empty()).then_some(program);
    }
    let ends = command
        .match_indices(char::is_whitespace)
        .map(|(end, _)| end)
        .chain([command.len()]);
    let mut first_word = None;
    for end in ends {
        let candidate = &command[..end];
        if candidate.is_empty() || candidate.ends_with(char::is_whitespace) {
            continue;
        }
        if has_exe_extension(candidate) {
            return Some(candidate);
        }
        first_word.get_or_insert(candidate);
    }
    first_word
}

fn has_exe_extension(path: &str) -> bool {
    path.len() > EXE_EXTENSION.len()
        && path
            .get(path.len() - EXE_EXTENSION.len()..)
            .is_some_and(|tail| tail.eq_ignore_ascii_case(EXE_EXTENSION))
}

/// Whether `command` starts the app binary under its current or its earlier
/// name, in any case, with or without `.exe`.
fn starts_app_binary(command: &str) -> bool {
    let Some(program) = run_command_program(command) else {
        return false;
    };
    let file_name = program.rsplit(['\\', '/']).next().unwrap_or(program);
    let stem = if has_exe_extension(file_name) {
        &file_name[..file_name.len() - EXE_EXTENSION.len()]
    } else {
        file_name
    };
    stem.eq_ignore_ascii_case(GUI_BINARY_STEM)
        || stem.eq_ignore_ascii_case(legacy_names::GUI_BINARY_STEM)
}

/// What moving the login item takes, given what the registry holds. A value
/// under the old name is Jotra's only when its program is the app binary
/// under either name and its command passes the login argument; anything
/// else is left alone.
pub fn plan_run_value(facts: &RunValueFacts<'_>) -> RunValuePlan {
    if facts.current_name == legacy_names::PRODUCT_NAME {
        return RunValuePlan::Untouched;
    }
    let Some(command) = facts.legacy_command else {
        return RunValuePlan::Untouched;
    };
    if !is_login_command_line(command) || !starts_app_binary(command) {
        return RunValuePlan::Untouched;
    }
    if facts.current_present {
        return RunValuePlan::RemoveLegacy;
    }
    RunValuePlan::Move {
        command: command.to_owned(),
        approval: facts.legacy_approval.map(<[u8]>::to_vec),
    }
}

/// The Application User Model ID the app runs under, kept from before the
/// rename so its windows group with taskbar pins made from either shortcut.
pub const APP_USER_MODEL_ID: &str = crate::bundle_rename::BUNDLE_IDENTIFIER;

/// `value` as a nul-terminated UTF-16 string, the form Windows APIs take.
pub fn wide_nul(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
