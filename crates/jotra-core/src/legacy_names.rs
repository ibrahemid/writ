//! The spellings the app used before its rename to Jotra (ADR-059 decision 5).
//!
//! Existing installs, notes, shell profiles and chat histories still hold these
//! values, so every reader that meets one accepts it beside the new spelling.
//! Nothing new is written under them. This module is the one Rust source file
//! that spells the old name; every other file names these constants.

/// The data folder under the home folder on installs made before the rename.
pub const DATA_FOLDER: &str = ".writ";

/// The index file inside [`DATA_FOLDER`], and the one an override folder may
/// still hold.
pub const INDEX_FILE: &str = "writ.db";

/// The notes folder default for the legacy lineage, under the home folder or
/// inside a data-folder override.
pub const NOTES_FOLDER: &str = "Writ";

/// The start every environment variable name the app read carried.
pub const VARIABLE_PREFIX: &str = "WRIT_";

/// The variable that named the data folder.
pub const DATA_DIR_VAR: &str = "WRIT_DATA_DIR";

/// The variable that named the notes folder.
pub const NOTES_DIR_VAR: &str = "WRIT_NOTES_DIR";

/// The variable that named the app binary the command launches.
pub const GUI_BIN_VAR: &str = "WRIT_GUI_BIN";

/// The link scheme notes, bookmarks and scripts may still hold.
pub const LINK_SCHEME: &str = "writ";

/// The info-string word chat replies stored before the rename open a proposal
/// with.
pub const PROPOSAL_WORD: &str = "writ-proposal";

/// The start of a minted note stem: `writ-<yymmdd>-<hhmm>`, or `writ-<millis>`
/// from before 0.4.
pub const MINTED_PREFIX: &str = "writ-";

/// The start of the temp file a save renames into place, as a crash may have
/// left it beside a note.
pub const TEMP_FILE_PREFIX: &str = ".writ-tmp-";

/// The tool name activity rows recorded for showing a file in the app.
pub const OPEN_IN_APP_ACTION: &str = "open_in_writ";

/// The second name the command ships under. Shell profiles, scripts and
/// assistant registrations made before the rename run it.
pub const COMMAND_NAME: &str = "writ";

/// The file stem the command's binary carried in channels that start it
/// through a wrapper script.
pub const CLI_BINARY_STEM: &str = "writ-cli";

/// The server name registrations made before the rename carry, and the name
/// the legacy dialect reports.
pub const SERVER_NAME: &str = "writ";

/// The tool the legacy dialect lists for showing a file in the app.
pub const OPEN_IN_APP_TOOL: &str = OPEN_IN_APP_ACTION;

/// The resource scheme the legacy dialect lists the notes folder under.
pub const RESOURCE_SCHEME: &str = "writ-folder";

/// The client name the connection test introduced itself with.
pub const PROBE_CLIENT_NAME: &str = "writ-connection-test";

/// The manifest `name` of the Claude Desktop bundle, kept so the installed
/// extension updates in place.
pub const BUNDLE_NAME: &str = "writ";

/// The variable that named the command a bundle launcher runs.
pub const COMMAND_VAR: &str = "WRIT_COMMAND";

/// The file stem of the app binary beside the command on installs made before
/// the rename.
pub const GUI_BINARY_STEM: &str = "writ-tauri";

/// The macOS app bundle's folder name on installs made before the rename, and
/// the hidden link the renamed bundle leaves at that path.
pub const BUNDLE_FOLDER: &str = "Writ.app";

/// The file name, in `~/Library/LaunchAgents`, of the macOS login item made
/// before the rename.
pub const LOGIN_AGENT_FILE: &str = "Writ.plist";

/// The product name the Windows and Linux login items were registered under
/// before the rename: the `Run` value name and the autostart file stem.
pub const PRODUCT_NAME: &str = "Writ";

/// The Linux desktop entry the packages shipped before the rename.
pub const DESKTOP_ENTRY: &str = "Writ.desktop";

/// The lower-case desktop entry id distribution packages and AppImage
/// integrations used before the rename.
pub const LOWERCASE_DESKTOP_ENTRY: &str = "writ.desktop";
