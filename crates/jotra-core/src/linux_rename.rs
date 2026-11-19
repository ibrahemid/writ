//! The Linux side of the rename (ADR-059 decision 11): the autostart entry,
//! and the command copies and desktop entry an AppImage keeps current.
//!
//! The decisions are here, over the text and paths the adapter reads; the
//! adapter reads the files and carries the plans out.

use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::cli_install::{carries_cli_signature, COMMAND_NAME, USER_BIN};
use crate::legacy_names;
use crate::startup::{is_login_command_line, GUI_BINARY_STEM};

/// The folder, under the home folder, the login item's desktop entry lives in.
pub const AUTOSTART_FOLDER: &str = ".config/autostart";

/// The file name `install.sh` gives the command it unpacks beside the image.
pub const CLI_BINARY_STEM: &str = "jotra-cli";

/// The desktop entry an AppImage integration made before the rename, in the
/// user's applications folder.
pub const APPIMAGE_ENTRY_FILE: &str = legacy_names::LOWERCASE_DESKTOP_ENTRY;

const DESKTOP_ENTRY_GROUP: &str = "[Desktop Entry]";
const EXEC_KEY: &str = "Exec";
const NAME_KEY: &str = "Name";
const COMMENT_KEY: &str = "Comment";
const AUTOSTART_COMMENT_SUFFIX: &str = " startup script";

/// The autostart file name the login item for `product_name` carries.
pub fn autostart_file_name(product_name: &str) -> String {
    format!("{product_name}.desktop")
}

/// The user's applications folder: `$XDG_DATA_HOME/applications`, else
/// `~/.local/share/applications`, or `None` with neither.
pub fn user_applications_dir(data_home: Option<&str>, home: Option<&Path>) -> Option<PathBuf> {
    match data_home.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) => Some(PathBuf::from(value).join("applications")),
        None => home.map(|home| home.join(".local/share/applications")),
    }
}

fn key_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (name, value) = line.split_once('=')?;
    (name.trim_end() == key).then(|| value.trim_start())
}

/// The value of `key` in the `[Desktop Entry]` group of `entry`, as written.
pub fn desktop_entry_value<'a>(entry: &'a str, key: &str) -> Option<&'a str> {
    let mut in_group = false;
    for line in entry.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_group = line == DESKTOP_ENTRY_GROUP;
            continue;
        }
        if in_group {
            if let Some(value) = key_value(line, key) {
                return Some(value);
            }
        }
    }
    None
}

/// `entry` with `key` set to `value` in its `[Desktop Entry]` group, every
/// other line kept as it was. A key the group lacks is added at the group's
/// end. `None` when `entry` has no `[Desktop Entry]` group.
pub fn set_desktop_entry_value(entry: &str, key: &str, value: &str) -> Option<String> {
    let mut out = String::with_capacity(entry.len() + key.len() + value.len() + 2);
    let mut in_group = false;
    let mut seen_group = false;
    let mut written = false;
    for line in entry.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let ending = &line[body.len()..];
        let trimmed = body.trim();
        if trimmed.starts_with('[') {
            if in_group && !written {
                out.push_str(&format!("{key}={value}\n"));
                written = true;
            }
            in_group = trimmed == DESKTOP_ENTRY_GROUP;
            seen_group |= in_group;
            out.push_str(line);
            continue;
        }
        if in_group && !written && key_value(trimmed, key).is_some() {
            out.push_str(&format!("{key}={value}{ending}"));
            written = true;
            continue;
        }
        out.push_str(line);
    }
    if !seen_group {
        return None;
    }
    if !written {
        if out.ends_with('\n') {
            out.push_str(&format!("{key}={value}\n"));
        } else {
            out.push_str(&format!("\n{key}={value}"));
        }
    }
    Some(out)
}

struct ExecWord {
    range: Range<usize>,
    text: String,
}

fn is_exec_space(ch: char) -> bool {
    matches!(ch, ' ' | '\t')
}

fn exec_words(exec: &str) -> Option<Vec<ExecWord>> {
    let mut words = Vec::new();
    let mut chars = exec.char_indices().peekable();
    while let Some(&(start, first)) = chars.peek() {
        if is_exec_space(first) {
            chars.next();
            continue;
        }
        let mut text = String::new();
        let end = if first == '"' {
            chars.next();
            loop {
                let (index, ch) = chars.next()?;
                match ch {
                    '\\' => text.push(chars.next()?.1),
                    '"' => break index + 1,
                    _ => text.push(ch),
                }
            }
        } else {
            loop {
                match chars.peek() {
                    Some(&(index, ch)) if is_exec_space(ch) => break index,
                    Some(&(_, ch)) => {
                        text.push(ch);
                        chars.next();
                    }
                    None => break exec.len(),
                }
            }
        };
        words.push(ExecWord {
            range: start..end,
            text,
        });
    }
    Some(words)
}

/// Where `path` is written in `exec` from `word` on: the word itself when it
/// reads as `path`, or the unquoted run of words that spells a path with
/// spaces as the autostart plugin writes it.
fn path_range(exec: &str, word: &ExecWord, path: &str) -> Option<Range<usize>> {
    if word.text == path {
        return Some(word.range.clone());
    }
    let start = word.range.start;
    let end = start + path.len();
    let written_bare = !exec[start..].starts_with('"') && exec[start..].starts_with(path);
    (written_bare && exec[end..].chars().next().is_none_or(is_exec_space)).then_some(start..end)
}

fn names_app_binary(word: &str) -> bool {
    let file_name = match word.rsplit_once('/') {
        Some((_, name)) if word.starts_with('/') => name,
        Some(_) => return false,
        None => word,
    };
    file_name == GUI_BINARY_STEM || file_name == legacy_names::GUI_BINARY_STEM
}

fn app_word_range(exec: &str, words: &[ExecWord], app_path: Option<&str>) -> Option<Range<usize>> {
    words.iter().find_map(|word| {
        app_path
            .and_then(|path| path_range(exec, word, path))
            .or_else(|| names_app_binary(&word.text).then(|| word.range.clone()))
    })
}

fn splice_exec(exec: &str, range: Range<usize>, word: &str) -> String {
    format!("{}{word}{}", &exec[..range.start], &exec[range.end..])
}

/// `program` written as a word of an `Exec` value: quoted when it
/// holds a reserved character, or `None` when it holds one that would need
/// escaping (`"`, `` ` ``, `$`, `\`, `%` or a control character).
pub fn exec_argument(program: &str) -> Option<String> {
    let needs_escape = |ch: char| matches!(ch, '"' | '`' | '$' | '\\' | '%') || ch.is_control();
    if program.is_empty() || program.chars().any(needs_escape) {
        return None;
    }
    let needs_quotes = |ch: char| {
        matches!(
            ch,
            ' ' | '\'' | '>' | '<' | '~' | '|' | '&' | ';' | '*' | '?' | '#' | '(' | ')'
        )
    };
    if program.chars().any(needs_quotes) {
        Some(format!("\"{program}\""))
    } else {
        Some(program.to_owned())
    }
}

/// `exec` with its first word that starts the app, `app_path` itself or the
/// app binary under either name, replaced by `app_path`; every other word is
/// kept as written. `None` when a quote is left open, no word starts the
/// app, or `app_path` cannot be written into an `Exec` value.
pub fn exec_with_app_path(exec: &str, app_path: &str) -> Option<String> {
    let words = exec_words(exec)?;
    let range = app_word_range(exec, &words, Some(app_path))?;
    Some(splice_exec(exec, range, &exec_argument(app_path)?))
}

/// What the autostart folder holds, as the plan needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutostartFacts<'a> {
    /// The text of the autostart entry under the old name, when there is one.
    pub legacy_entry: Option<&'a str>,
    /// Whether an entry under the current name already exists.
    pub current_present: bool,
    /// The product name this build registers its login item under.
    pub app_name: &'a str,
    /// The path the login item starts, as the autostart plugin writes it, or
    /// `None` when it is not UTF-8.
    pub app_path: Option<&'a str>,
}

/// What moving the autostart entry takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AutostartPlan {
    /// Nothing to move: no entry under the old name, one without the login
    /// argument, or a build that still runs under the old name.
    Untouched,
    /// The old entry passes the login argument, but no word of its `Exec`
    /// starts the app or a quote in it is left open; nothing moves.
    ForeignProgram,
    /// An entry under the current name already exists; the old one goes so
    /// login starts the app once and the app's own switch governs it.
    RemoveLegacy,
    /// The app's path cannot be written into an `Exec` value; nothing moves.
    PathNotWritable,
    /// Write `entry` under the current name, then remove the old file.
    Replace {
        /// The old entry with the new `Exec`; `Hidden`,
        /// `X-GNOME-Autostart-enabled` and every other key kept.
        entry: String,
    },
}

/// What moving the autostart entry takes. An entry under the old name is
/// Jotra's only when its `Exec` passes the login argument and a word of it
/// starts the app: `app_path` itself or the app binary under either name.
/// Only that word changes. `Name` and `Comment` follow the new name only
/// while they still read as the autostart plugin wrote them.
pub fn plan_autostart(facts: &AutostartFacts<'_>) -> AutostartPlan {
    if facts.app_name == legacy_names::PRODUCT_NAME {
        return AutostartPlan::Untouched;
    }
    let Some(legacy) = facts.legacy_entry else {
        return AutostartPlan::Untouched;
    };
    let Some(exec) = desktop_entry_value(legacy, EXEC_KEY) else {
        return AutostartPlan::Untouched;
    };
    if !is_login_command_line(exec) {
        return AutostartPlan::Untouched;
    }
    let Some(app_word) =
        exec_words(exec).and_then(|words| app_word_range(exec, &words, facts.app_path))
    else {
        return AutostartPlan::ForeignProgram;
    };
    if facts.current_present {
        return AutostartPlan::RemoveLegacy;
    }
    let Some(new_exec) = facts
        .app_path
        .and_then(exec_argument)
        .map(|path| splice_exec(exec, app_word, &path))
    else {
        return AutostartPlan::PathNotWritable;
    };
    let Some(mut entry) = set_desktop_entry_value(legacy, EXEC_KEY, &new_exec) else {
        return AutostartPlan::Untouched;
    };
    if desktop_entry_value(&entry, NAME_KEY) == Some(legacy_names::PRODUCT_NAME) {
        entry = set_desktop_entry_value(&entry, NAME_KEY, facts.app_name).unwrap_or(entry);
    }
    let legacy_comment = format!("{}{AUTOSTART_COMMENT_SUFFIX}", legacy_names::PRODUCT_NAME);
    if desktop_entry_value(&entry, COMMENT_KEY) == Some(legacy_comment.as_str()) {
        let comment = format!("{}{AUTOSTART_COMMENT_SUFFIX}", facts.app_name);
        entry = set_desktop_entry_value(&entry, COMMENT_KEY, &comment).unwrap_or(entry);
    }
    AutostartPlan::Replace { entry }
}

/// The AppImage's desktop entry `entry` with its `Name` set to `app_name` and
/// the image in its `Exec` written as the spec asks, when its `Exec` starts
/// the running `image`; `None` when the entry is another program's or
/// already current.
pub fn plan_appimage_entry(entry: &str, image: &str, app_name: &str) -> Option<String> {
    let exec = desktop_entry_value(entry, EXEC_KEY)?;
    let words = exec_words(exec)?;
    let range = path_range(exec, words.first()?, image)?;
    let exec = splice_exec(exec, range, &exec_argument(image)?);
    let updated = set_desktop_entry_value(entry, EXEC_KEY, &exec)?;
    let updated = set_desktop_entry_value(&updated, NAME_KEY, app_name)?;
    (updated != entry).then_some(updated)
}

/// A place an AppImage install may have copied the command to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandCopy {
    /// The copy's path.
    pub path: PathBuf,
    /// Whether it carries the command's name from before the rename, and so
    /// is refreshed from the command bundled under that name when there is one.
    pub legacy_name: bool,
}

/// The command copies an AppImage install may have made: the command under
/// its current and its earlier name in `~/.local/bin`, and the command
/// `install.sh` unpacks beside the image under either name.
pub fn appimage_command_copies(home: Option<&Path>, image: &Path) -> Vec<CommandCopy> {
    let mut copies = Vec::with_capacity(4);
    if let Some(home) = home {
        let bin = home.join(USER_BIN);
        copies.push(CommandCopy {
            path: bin.join(legacy_names::COMMAND_NAME),
            legacy_name: true,
        });
        copies.push(CommandCopy {
            path: bin.join(COMMAND_NAME),
            legacy_name: false,
        });
    }
    if let Some(folder) = image.parent() {
        copies.push(CommandCopy {
            path: folder.join(legacy_names::CLI_BINARY_STEM),
            legacy_name: true,
        });
        copies.push(CommandCopy {
            path: folder.join(CLI_BINARY_STEM),
            legacy_name: false,
        });
    }
    copies
}

/// Whether a copy holding `copy` is a build of the command other than the
/// `bundled` one the running image carries, and so is refreshed.
pub fn is_stale_command_copy(copy: &[u8], bundled: &[u8]) -> bool {
    carries_cli_signature(copy) && copy != bundled
}
