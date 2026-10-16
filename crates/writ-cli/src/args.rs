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
A <FILE> is a path, or the name of a file in your folder. A path to open may end
in :LINE, as in Plan.md:12, to open at that line.
rename does not yet rewrite links that name the file by its old name.
Exit codes: 0 done, 1 nothing to read or the operation failed, 2 bad arguments.

As your editor: git config --global core.editor \"writ --wait\"

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
    bin_name = "writ",
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

    /// Return only when every file opened has been closed in Writ.
    #[arg(long, conflicts_with = "no_open")]
    pub wait: bool,

    /// Save piped stdin and print its path without opening Writ.
    #[arg(long)]
    pub no_open: bool,
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
    /// Files whose text matches a search, best match first
    Search {
        /// What to look for
        #[arg(value_name = "QUERY")]
        query: String,
        /// How many files to list
        #[arg(long, value_name = "N", default_value_t = DEFAULT_SEARCH_LIMIT)]
        limit: usize,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Print a file's text
    Cat {
        /// A path, or the name of a file in your folder
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Every file in your folder, or in one folder inside it
    Ls {
        /// A folder inside your folder, such as projects/
        #[arg(value_name = "FOLDER")]
        folder: Option<String>,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Add piped stdin to the end of a file, keeping a version of what it held
    Append {
        /// A path, or the name of a file in your folder; a name nothing has
        /// yet is created
        #[arg(value_name = "FILE")]
        file: String,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Open today's file, creating it when the day has none
    Today {
        /// Add piped stdin to the end of today's file first
        #[arg(long)]
        append: bool,
        /// Print the path without opening Writ
        #[arg(long)]
        no_open: bool,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Open a file in your folder by name, at a line with NAME:LINE
    Open {
        /// The name of a file in your folder, or a path
        #[arg(value_name = "NAME")]
        name: String,
        /// Return only when the file has been closed in Writ
        #[arg(long)]
        wait: bool,
        /// Print a JSON document
        #[arg(long)]
        json: bool,
    },
    /// Create a file in your folder, holding piped stdin when there is any
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

/// How many files `writ search` lists unless told otherwise.
pub const DEFAULT_SEARCH_LIMIT: usize = 20;

/// What a command line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Invocation {
    /// Open these paths, or read piped stdin when there are none.
    Open {
        /// The paths as given.
        paths: Vec<OsString>,
        /// The name piped stdin is saved under.
        title: Option<String>,
        /// Return once every file opened has been closed.
        wait: bool,
        /// Save piped stdin without opening it.
        no_open: bool,
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
                wait: cli.wait,
                no_open: cli.no_open,
            })
        }
        Some(Command::Mcp) => return Ok(Invocation::Mcp),
        Some(Command::Links { file, json }) => Verb::Links { note: file, json },
        Some(Command::Backlinks { file, json }) => Verb::Backlinks { note: file, json },
        Some(Command::Properties { file, json }) => Verb::Properties { note: file, json },
        Some(Command::Tags { file, json }) => Verb::Tags { note: file, json },
        Some(Command::Search { query, limit, json }) => Verb::Search { query, limit, json },
        Some(Command::Cat { file, json }) => Verb::Cat { note: file, json },
        Some(Command::Ls { folder, json }) => Verb::Ls { folder, json },
        Some(Command::Append { file, json }) => Verb::Append { note: file, json },
        Some(Command::Today {
            append,
            no_open,
            json,
        }) => Verb::Today {
            append,
            open: !no_open,
            json,
        },
        Some(Command::Open { name, wait, json }) => Verb::Open {
            note: name,
            wait,
            json,
        },
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

    const VERBS: [&str; 13] = [
        "links",
        "backlinks",
        "properties",
        "tags",
        "search",
        "cat",
        "ls",
        "append",
        "today",
        "open",
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
                title: None,
                wait: false,
                no_open: false,
            }
        );
        assert_eq!(
            parse(&["-", "--title", "out"]).unwrap(),
            Invocation::Open {
                paths: vec!["-".into()],
                title: Some("out".to_string()),
                wait: false,
                no_open: false,
            }
        );
        assert_eq!(
            parse(&[]).unwrap(),
            Invocation::Open {
                paths: vec![],
                title: None,
                wait: false,
                no_open: false,
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
    fn wait_and_no_open_belong_to_the_file_opening_path() {
        assert_eq!(
            parse(&["--wait", "msg.txt"]).unwrap(),
            Invocation::Open {
                paths: vec!["msg.txt".into()],
                title: None,
                wait: true,
                no_open: false,
            }
        );
        assert_eq!(
            parse(&["--no-open", "--title", "log"]).unwrap(),
            Invocation::Open {
                paths: vec![],
                title: Some("log".to_string()),
                wait: false,
                no_open: true,
            }
        );
        let both = parse(&["--wait", "--no-open", "a"]).unwrap_err();
        assert_eq!(both.exit_code(), crate::verbs::EXIT_USAGE);
    }

    #[test]
    fn the_host_verbs_read_their_arguments() {
        assert_eq!(
            parse(&["search", "launch plan", "--limit", "5", "--json"]).unwrap(),
            Invocation::Verb(Verb::Search {
                query: "launch plan".to_string(),
                limit: 5,
                json: true
            })
        );
        assert_eq!(
            parse(&["search", "x"]).unwrap(),
            Invocation::Verb(Verb::Search {
                query: "x".to_string(),
                limit: DEFAULT_SEARCH_LIMIT,
                json: false
            })
        );
        assert_eq!(
            parse(&["ls"]).unwrap(),
            Invocation::Verb(Verb::Ls {
                folder: None,
                json: false
            })
        );
        assert_eq!(
            parse(&["today", "--append", "--no-open"]).unwrap(),
            Invocation::Verb(Verb::Today {
                append: true,
                open: false,
                json: false
            })
        );
        assert_eq!(
            parse(&["open", "Plan:12", "--wait"]).unwrap(),
            Invocation::Verb(Verb::Open {
                note: "Plan:12".to_string(),
                wait: true,
                json: false
            })
        );
        let limit = parse(&["search", "x", "--limit", "many"]).unwrap_err();
        assert_eq!(limit.exit_code(), crate::verbs::EXIT_USAGE);
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
    fn the_usage_names_writ_whatever_file_runs_it() {
        for program in ["writ.exe", "writ-aarch64-apple-darwin"] {
            let missing = parse_invocation([program, "links"])
                .unwrap_err()
                .render()
                .to_string();
            assert!(
                missing.contains("Usage: writ links <FILE>"),
                "{program}: {missing}"
            );
            let help = parse_invocation([program, "--help"])
                .unwrap_err()
                .render()
                .to_string();
            assert!(help.contains("Usage: writ "), "{program}: {help}");
            assert!(!help.contains(program), "{program}: {help}");
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
        assert!(
            help.contains("Settings, Apps, Connected programs"),
            "{help}"
        );
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
