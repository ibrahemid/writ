//! `writ mcp`: the MCP server, over this process's stdin and stdout.
//!
//! A client launches this command and speaks the protocol down the pipe. The
//! app hosts nothing: a window has no client stdio to attach to, so "shipped
//! with the app" means the app ships this binary and shows the command to paste
//! into a client's configuration (ADR-031 rule 2.4).
//!
//! The server reads `writ.db` read-only and creates nothing. Two processes do
//! not write one SQLite file, so a folder Writ has never opened has no index
//! and the index-derived tools say so.

use std::ffi::OsString;
use std::path::Path;

/// The verb this module answers to.
pub const VERB: &str = "mcp";

/// What the command line asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Serve on stdio.
    Serve,
    /// Print the help text.
    Help,
}

/// Why a `writ mcp` command line could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum UsageError {
    /// A flag the command does not take.
    #[error("writ mcp does not take {flag}")]
    UnknownFlag {
        /// The flag as it was written.
        flag: String,
    },
    /// A positional argument the command does not take.
    #[error("writ mcp takes no arguments")]
    TooManyArguments,
    /// An argument that is not valid text.
    #[error("writ mcp was given an argument that is not text")]
    NotText,
}

/// The help text, printed for `writ mcp --help`.
pub fn help() -> String {
    [
        "Usage: writ mcp",
        "",
        "Serves the notes folder to an MCP client over stdin and stdout.",
        "The client launches this command; it opens no port.",
        "",
        "Turn it on with `enabled = true` under [mcp] in config.toml.",
        "Until then every tool call is refused.",
        "",
        "Environment:",
        "  WRIT_NOTES_DIR  The notes folder to read, overriding the setting.",
        "  WRIT_DATA_DIR   The folder holding writ.db and config.toml.",
    ]
    .join("\n")
}

/// Reads a `writ mcp` invocation off the front of `args`, or `None` when the
/// first argument does not name this verb.
pub fn parse(args: &[OsString]) -> Option<Result<Command, UsageError>> {
    if args.first()?.to_str()? != VERB {
        return None;
    }
    Some(parse_rest(&args[1..]))
}

fn parse_rest(rest: &[OsString]) -> Result<Command, UsageError> {
    let mut command = Command::Serve;
    for arg in rest {
        let Some(text) = arg.to_str() else {
            return Err(UsageError::NotText);
        };
        match text {
            "--help" | "-h" => command = Command::Help,
            flag if flag.starts_with('-') => {
                return Err(UsageError::UnknownFlag {
                    flag: flag.to_string(),
                })
            }
            _ => return Err(UsageError::TooManyArguments),
        }
    }
    Ok(command)
}

/// Whether `[mcp] enabled` is set in `<writ_dir>/config.toml`.
///
/// Absent, unreadable or unparseable reads as off, which is the default the
/// config type carries: nothing here turns the server on by guessing.
pub fn server_is_enabled(writ_dir: &Path) -> bool {
    let Ok(text) = std::fs::read_to_string(writ_dir.join("config.toml")) else {
        return false;
    };
    let Ok(document) = toml::from_str::<toml::Table>(&text) else {
        return false;
    };
    document
        .get("mcp")
        .and_then(|section| section.get("enabled"))
        .and_then(|value| value.as_bool())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(parts: &[&str]) -> Vec<OsString> {
        parts.iter().map(OsString::from).collect()
    }

    #[test]
    fn another_verb_is_not_this_one() {
        assert!(parse(&args(&["links", "Launch"])).is_none());
        assert!(parse(&args(&[])).is_none());
    }

    #[test]
    fn the_bare_verb_serves() {
        assert_eq!(parse(&args(&["mcp"])), Some(Ok(Command::Serve)));
    }

    #[test]
    fn help_is_asked_for_by_either_spelling() {
        assert_eq!(parse(&args(&["mcp", "--help"])), Some(Ok(Command::Help)));
        assert_eq!(parse(&args(&["mcp", "-h"])), Some(Ok(Command::Help)));
    }

    #[test]
    fn an_unknown_flag_is_a_typed_error_naming_the_flag() {
        assert_eq!(
            parse(&args(&["mcp", "--port=3000"])),
            Some(Err(UsageError::UnknownFlag {
                flag: "--port=3000".to_string()
            }))
        );
    }

    #[test]
    fn an_argument_the_command_does_not_take_is_refused() {
        assert_eq!(
            parse(&args(&["mcp", "serve"])),
            Some(Err(UsageError::TooManyArguments))
        );
    }

    #[test]
    fn the_help_text_names_the_command_and_the_setting() {
        let help = help();
        assert!(help.contains("writ mcp"));
        assert!(help.contains("[mcp]"));
        assert!(help.contains("stdin and stdout"));
    }

    #[test]
    fn a_folder_with_no_config_reads_as_off() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        assert!(!server_is_enabled(dir.path()));
    }

    #[test]
    fn a_config_without_the_section_reads_as_off() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("config.toml"), "[ai]\nenabled = true\n").expect("seed");
        assert!(!server_is_enabled(dir.path()));
    }

    #[test]
    fn a_config_that_turns_it_on_reads_as_on() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("config.toml"), "[mcp]\nenabled = true\n").expect("seed");
        assert!(server_is_enabled(dir.path()));
    }

    #[test]
    fn a_config_that_is_not_toml_reads_as_off() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(dir.path().join("config.toml"), "[mcp\nenabled = true").expect("seed");
        assert!(!server_is_enabled(dir.path()));
    }
}
