//! Which names the MCP server answers under, chosen by the name the command
//! was started as (ADR-059 decision 7).
//!
//! Client approvals and permission lists key on the server name and the tool
//! name, so a registration made before the rename keeps working only while
//! the server it starts reports the names it was approved under. Both dialects
//! answer calls to either tool name and either resource scheme; they differ in
//! what they list.

use std::ffi::OsStr;

use crate::legacy_names;
use crate::mcp_clients::SERVER_NAME;

/// The variable a wrapper script sets to the command name it was run as, for
/// channels where the binary it starts carries another file name.
pub const COMMAND_NAME_VAR: &str = "JOTRA_COMMAND_NAME";

/// The tool the current dialect lists for showing a file in the app.
pub const OPEN_IN_APP_TOOL: &str = "open_in_jotra";

/// The resource scheme the current dialect lists the notes folder under.
pub const RESOURCE_SCHEME: &str = "jotra-folder";

/// The command file stems that select the legacy dialect.
const LEGACY_STEMS: [&str; 2] = [legacy_names::COMMAND_NAME, legacy_names::CLI_BINARY_STEM];

/// The names an MCP session answers under.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum Dialect {
    /// The names registrations made before the rename were approved under.
    Legacy,
    /// The names new registrations carry.
    #[default]
    Current,
}

impl Dialect {
    /// Every dialect, for a reader that accepts what either one lists.
    pub const ALL: [Dialect; 2] = [Dialect::Current, Dialect::Legacy];

    /// The `serverInfo.name` the server reports.
    pub fn server_name(self) -> &'static str {
        match self {
            Dialect::Legacy => legacy_names::SERVER_NAME,
            Dialect::Current => SERVER_NAME,
        }
    }

    /// The name the tool that shows a file in the app is listed under.
    pub fn listed_open_tool(self) -> &'static str {
        match self {
            Dialect::Legacy => legacy_names::OPEN_IN_APP_TOOL,
            Dialect::Current => OPEN_IN_APP_TOOL,
        }
    }

    /// The other name of that tool: answered when called, never listed.
    pub fn unlisted_open_tool(self) -> &'static str {
        match self {
            Dialect::Legacy => OPEN_IN_APP_TOOL,
            Dialect::Current => legacy_names::OPEN_IN_APP_TOOL,
        }
    }

    /// The scheme the server lists resources under.
    pub fn resource_scheme(self) -> &'static str {
        match self {
            Dialect::Legacy => legacy_names::RESOURCE_SCHEME,
            Dialect::Current => RESOURCE_SCHEME,
        }
    }
}

/// The dialect a session speaks.
///
/// `command_name` is [`COMMAND_NAME_VAR`] as the process received it, and
/// `argv0` the first argument it was started with. The variable decides when
/// it holds a name; otherwise the file stem of `argv0` does. The stems of the
/// legacy command and of the binary its wrappers start select
/// [`Dialect::Legacy`], in any case; any other stem, or none, selects
/// [`Dialect::Current`].
pub fn select_dialect(command_name: Option<&OsStr>, argv0: Option<&OsStr>) -> Dialect {
    let named = command_name.filter(|name| !name.to_string_lossy().trim().is_empty());
    let Some(invoked) = named.or(argv0) else {
        return Dialect::Current;
    };
    let stem = file_stem(&invoked.to_string_lossy()).to_ascii_lowercase();
    if LEGACY_STEMS.contains(&stem.as_str()) {
        Dialect::Legacy
    } else {
        Dialect::Current
    }
}

/// The file stem of `invoked`, split on either path separator so a Windows
/// path reads the same on every platform.
fn file_stem(invoked: &str) -> &str {
    let file_name = invoked
        .trim()
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    match file_name.rsplit_once('.') {
        Some((stem, _)) if !stem.is_empty() => stem,
        _ => file_name,
    }
}

/// Whether `name` is either name of the tool that shows a file in the app.
pub fn is_open_in_app_tool(name: &str) -> bool {
    name == OPEN_IN_APP_TOOL || name == legacy_names::OPEN_IN_APP_TOOL
}
