//! The command line: the file-opening path, the note verbs and `mcp`, read by
//! one clap parser so `writ --help` lists every one of them.
//!
//! A usage error is clap's: it prints what was wrong and the usage on stderr
//! and exits [`crate::verbs::EXIT_USAGE`].

use std::ffi::OsString;

use clap::{CommandFactory, Parser, Subcommand};

use crate::mcp;
use crate::verbs::Verb;

const ABOUT: &str = "Open files, folders, or piped input in Writ";

const AFTER_HELP: &str = "\
A <FILE> is a path, or the name of a file in your folder.
rename does not yet rewrite links that name the file by its old name.
Exit codes: 0 done, 1 nothing to read or the operation failed, 2 bad arguments.

Environment:
  WRIT_GUI_BIN    Path to the Writ application binary. Read on Linux and Windows,
                  where it takes precedence over the binary installed next to
                  this one.
  WRIT_NOTES_DIR  The folder to read and write, overriding the setting.
  WRIT_DATA_DIR   The folder holding writ.db and config.toml.";

/// The whole command line.
#[derive(Debug, Parser)]
#[command(
    name = "writ",
    about = ABOUT,
    after_help = AFTER_HELP,
    version,
    args_conflicts_with_subcommands = true,
    disable_help_subcommand = true
)]
pub struct Cli {
    /// A note verb or `mcp`; with none, the paths are opened.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Files or directories to open. Pass `-` for piped stdin, or omit to open Writ.
    #[arg(value_name = "PATH")]
    pub paths: Vec<OsString>,

    /// Name of the file piped stdin is saved to.
    #[arg(long, value_name = "TITLE")]
    pub title: Option<String>,
}

/// The subcommands, in the order `--help` lists them.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Links written in a file
    Links {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Links in other files pointing at a file
    Backlinks {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// A file's frontmatter properties
    Properties {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// A file's tags, or every tag in the folder
    Tags {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: Option<String>,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Create a file in your folder
    New {
        /// The file's name; with none, Writ names it
        #[arg(value_name = "NAME")]
        name: Option<String>,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Rename a file, keeping its folder
    Rename {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// The new name
        #[arg(value_name = "NEW_NAME")]
        new_name: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Move a file to the trash
    Trash {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Serve your folder to an MCP client over stdin and stdout
    #[command(long_about = mcp::LONG_ABOUT, after_help = mcp::ENV_HELP)]
    Mcp,
}

/// What a command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Open these paths, or read piped stdin when there are none.
    Open {
        /// The paths as given.
        paths: Vec<OsString>,
        /// The name piped stdin is saved under.
        title: Option<String>,
    },
    /// Run a note verb.
    Verb(Verb),
    /// Serve the MCP server on stdio.
    Mcp,
}

/// Reads a command line, `args` including the program name.
///
/// # Errors
///
/// clap's error for a command line it cannot read, and for `--help` and
/// `--version`, whose `exit` prints the text and ends the process.
pub fn parse_invocation<I, T>(args: I) -> Result<Invocation, clap::Error>
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let cli = Cli::try_parse_from(args)?;
    let verb = match cli.command {
        None => {
            return Ok(Invocation::Open {
                paths: cli.paths,
                title: cli.title,
            })
        }
        Some(Command::Mcp) => return Ok(Invocation::Mcp),
        Some(Command::Links { file, json }) => Verb::Links { note: file, json },
        Some(Command::Backlinks { file, json }) => Verb::Backlinks { note: file, json },
        Some(Command::Properties { file, json }) => Verb::Properties { note: file, json },
        Some(Command::Tags { file, json }) => Verb::Tags { note: file, json },
        Some(Command::New { name, json }) => Verb::New { name, json },
        Some(Command::Rename {
            file,
            new_name,
            json,
        }) => Verb::Rename {
            note: file,
            new_name,
            json,
        },
        Some(Command::Trash { file, json }) => Verb::Trash { note: file, json },
    };
    Ok(Invocation::Verb(verb))
}

/// The help `writ --help` prints, for tests and for callers that render it.
pub fn long_help() -> String {
    Cli::command().render_long_help().to_string()
}

/// The help `writ <name> --help` prints, or `None` for no such subcommand.
pub fn subcommand_help(name: &str) -> Option<String> {
    let mut command = Cli::command();
    command
        .find_subcommand_mut(name)
        .map(|subcommand| subcommand.render_long_help().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(args: &[&str]) -> Result<Invocation, clap::Error> {
        parse_invocation(std::iter::once("writ").chain(args.iter().copied()))
    }

    const VERBS: [&str; 7] = [
        "links",
        "backlinks",
        "properties",
        "tags",
        "new",
        "rename",
        "trash",
    ];

    #[test]
    fn the_parser_is_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn a_path_is_not_a_verb() {
        assert_eq!(
            parse(&["note.md", "./links"]).unwrap(),
            Invocation::Open {
                paths: vec!["note.md".into(), "./links".into()],
                title: None
            }
        );
        assert_eq!(
            parse(&["-", "--title", "out"]).unwrap(),
            Invocation::Open {
                paths: vec!["-".into()],
                title: Some("out".to_string())
            }
        );
        assert_eq!(
            parse(&[]).unwrap(),
            Invocation::Open {
                paths: vec![],
                title: None
            }
        );
    }

    #[test]
    fn a_read_verb_takes_a_file_and_the_json_flag_in_either_order() {
        for args in [["links", "Note", "--json"], ["links", "--json", "Note"]] {
            assert_eq!(
                parse(&args).unwrap(),
                Invocation::Verb(Verb::Links {
                    note: "Note".to_string(),
                    json: true
                })
            );
        }
        assert_eq!(
            parse(&["backlinks", "Note"]).unwrap(),
            Invocation::Verb(Verb::Backlinks {
                note: "Note".to_string(),
                json: false
            })
        );
    }

    #[test]
    fn tags_and_new_take_an_optional_argument() {
        assert_eq!(
            parse(&["tags"]).unwrap(),
            Invocation::Verb(Verb::Tags {
                note: None,
                json: false
            })
        );
        assert_eq!(
            parse(&["new", "Ideas", "--json"]).unwrap(),
            Invocation::Verb(Verb::New {
                name: Some("Ideas".to_string()),
                json: true
            })
        );
    }

    #[test]
    fn rename_takes_two_names() {
        assert_eq!(
            parse(&["rename", "Old", "New"]).unwrap(),
            Invocation::Verb(Verb::Rename {
                note: "Old".to_string(),
                new_name: "New".to_string(),
                json: false
            })
        );
        let missing = parse(&["rename", "Old"]).unwrap_err();
        assert_eq!(
            missing.kind(),
            clap::error::ErrorKind::MissingRequiredArgument
        );
        assert_eq!(missing.exit_code(), crate::verbs::EXIT_USAGE);
    }

    #[test]
    fn mcp_is_a_subcommand_that_takes_nothing() {
        assert_eq!(parse(&["mcp"]).unwrap(), Invocation::Mcp);
        let extra = parse(&["mcp", "serve"]).unwrap_err();
        assert_eq!(extra.exit_code(), crate::verbs::EXIT_USAGE);
        let flag = parse(&["mcp", "--port=3000"]).unwrap_err();
        assert_eq!(flag.exit_code(), crate::verbs::EXIT_USAGE);
    }

    #[test]
    fn every_usage_error_exits_two() {
        for args in [
            vec!["links"],
            vec!["links", "a", "b"],
            vec!["rename", "a", "b", "c"],
            vec!["tags", "--yaml"],
            vec!["--no-such-flag"],
        ] {
            let error = parse(&args).unwrap_err();
            assert_eq!(error.exit_code(), crate::verbs::EXIT_USAGE, "{args:?}");
        }
    }

    #[test]
    fn help_and_version_exit_zero() {
        for args in [["--help"], ["--version"]] {
            assert_eq!(parse(&args).unwrap_err().exit_code(), 0, "{args:?}");
        }
        assert_eq!(parse(&["links", "--help"]).unwrap_err().exit_code(), 0);
    }

    #[test]
    fn the_help_lists_every_verb_and_mcp_with_what_it_does() {
        let help = long_help();
        for name in VERBS.iter().chain(&["mcp"]) {
            let line = help
                .lines()
                .find(|line| line.trim_start().starts_with(&format!("{name} ")))
                .unwrap_or_else(|| panic!("no help line for {name}:\n{help}"));
            assert!(line.split_whitespace().count() > 1, "{name} has no summary");
        }
        assert!(help.contains("Exit codes"), "{help}");
    }

    #[test]
    fn every_verb_offers_json() {
        for name in VERBS {
            let help = subcommand_help(name).expect("a subcommand");
            assert!(help.contains("--json"), "{name} takes --json:\n{help}");
        }
    }

    #[test]
    fn the_mcp_help_names_where_it_is_turned_on() {
        let help = subcommand_help("mcp").expect("mcp");
        assert!(help.contains("Connected programs"), "{help}");
        assert!(help.contains("stdin and stdout"), "{help}");
        assert!(help.contains("WRIT_NOTES_DIR"), "{help}");
    }

    #[test]
    fn the_help_uses_plain_words() {
        let mut texts = vec![long_help()];
        texts.extend(
            VERBS
                .iter()
                .chain(&["mcp"])
                .filter_map(|name| subcommand_help(name)),
        );
        for text in texts {
            let lower = text.to_lowercase();
            for banned in ["vault", "buffer", "scratchpad", "second brain"] {
                assert!(!lower.contains(banned), "help says {banned}");
            }
        }
    }
}
