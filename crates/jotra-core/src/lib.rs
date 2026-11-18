//! Pure domain logic for the Jotra editor.
//!
//! `jotra-core` is the policy layer of Jotra: it defines the buffer model,
//! configuration schema, conflict resolution, command registry, history,
//! and event payloads. It has no knowledge of Tauri, SQLite, the
//! filesystem, or any specific runtime.
//!
//! # Crate boundary
//!
//! This crate is intentionally framework-free. It depends only on `serde`,
//! `tracing`, and small utility crates. Persistence lives in the sibling
//! `jotra-storage` crate, and the IPC adapter lives in `jotra-tauri`.
//!
//! # Module layout
//!
//! - [`ai`]: the provider table and the model-list parsers (ADR-040).
//! - [`buffer`]: document model and the in-memory [`buffer::BufferManager`].
//! - [`bundle_rename`]: whether the macOS bundle moves to its new name
//!   (ADR-059).
//! - [`chat`]: request shape and stream grammar for the chat pane (ADR-031).
//! - [`cli_install`]: where the `jotra` command is installed (ADR-017).
//! - [`command`]: command registry types for palette-driven actions.
//! - [`config`]: typed user configuration with serde defaults.
//! - [`crash`]: what a panic report may say (ADR-045).
//! - [`data_folder`]: which data folder a launch uses (ADR-059).
//! - [`dictation`]: speech segmentation, languages, presets and the events
//!   and errors of a dictation session.
//! - [`errors`]: crate-wide [`errors::JotraError`] / [`errors::JotraResult`].
//! - [`events`]: strongly-typed domain events and an in-process event bus.
//! - [`file_ops`]: pure helpers for file validation and the open tiers.
//! - [`hash`]: SHA-256 helpers shared by the watcher, the write guard and
//!   the notes migration.
//! - [`history`]: recently-closed buffer history.
//! - [`hotkey`]: platform-neutral chord representation and parser.
//! - [`inbox`]: watch-inbox auto-open policy (ADR-018).
//! - [`legacy_names`]: the spellings from before the rename (ADR-059).
//! - [`link`]: external-link allowlist policy (ADR-025).
//! - [`linux_rename`]: the Linux autostart entry, AppImage command copies and
//!   desktop entry after the rename (ADR-059).
//! - [`maintenance`]: database bloat policy driving startup `VACUUM`.
//! - [`mcp_clients`]: the MCP clients Jotra connects, and the command, link,
//!   bundle or JSON each is handed (ADR-031 rule 2.4).
//! - [`mcp_dialect`]: the names the MCP server answers under, by the name the
//!   command was started as (ADR-059).
//! - [`name_migration`]: the per-user record of the rename's one-time steps.
//! - [`notes`]: notes-folder policy — where the folder is, how a title
//!   becomes a filename, whether a save may land on the file it is aimed
//!   at (ADR-028), and how a note links to another note (ADR-034).
//! - [`prompt`]: prompt-document helpers — token estimation, stripping,
//!   placeholders (ADR-015).
//! - [`recovery`]: crash-recovery policy types and resolution logic.
//! - [`serial`]: per-key queues that keep one note's commands in order
//!   off the main thread (ADR-047).
//! - [`startup`]: startup-failure report formatting and location policy.
//! - [`text_encoding`]: which text encoding a file is in, and reading and
//!   writing it in that encoding (ADR-049).
//! - [`title_bar`]: what a double-click on the window's title bar does.
//! - [`tools`]: the tools a connected program can call (ADR-031, ADR-032).
//! - [`update`]: update lifecycle phases and transition policy.
//! - [`user_variables`]: the environment variables a person sets, under both
//!   names.
//! - [`watcher`]: external-change representation and ignore stamps.
//! - [`windows_rename`]: the Windows login item and app id after the rename
//!   (ADR-059).
//! - [`workspace`]: workspace-level state (reserved).
//! - [`jotra_link`]: the `jotra://` link grammar, an allowlist that never
//!   writes (ADR-052).

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![warn(rustdoc::broken_intra_doc_links)]

/// What Jotra did on someone else's behalf, and what it decided about it.
pub mod activity;
/// The AI connection: the provider table and the model-list parsers.
pub mod ai;
/// Buffer document model and in-memory manager.
pub mod buffer;
/// Whether the macOS bundle moves to its new name, and the steps after it.
pub mod bundle_rename;
/// What the quick capture note adds to Today's File.
pub mod capture;
/// Request shape and stream grammar for the chat pane.
pub mod chat;
/// Where the `jotra` command is installed and which one a client is handed.
pub mod cli_install;
/// Command registry types for the command palette.
pub mod command;
/// Typed configuration schema with serde defaults.
pub mod config;
/// What a panic report may say, and the file it goes to (ADR-045).
pub mod crash;
/// Which data folder a launch uses, and the notes default that follows from it.
pub mod data_folder;
/// Default-app policy: UTI mapping and handler-status types.
pub mod default_app;
/// Dictation policy: segmentation, languages, presets, events and errors.
pub mod dictation;
/// Line diff for what a proposal would change.
pub mod diff;
/// Crate-wide error and result types.
pub mod errors;
/// Strongly-typed domain events and an in-process event bus.
pub mod events;
/// Pure file helpers: the open tiers, filename extraction, hex dumps.
pub mod file_ops;
/// SHA-256 helpers shared by the watcher, the write guard and the migration.
pub mod hash;
/// Recently-closed buffer history types.
pub mod history;
/// Platform-neutral hotkey chord representation and parser.
pub mod hotkey;
/// Watch-inbox auto-open policy — ADR-018.
pub mod inbox;
/// One running Jotra per data folder, and what a second launch hands it.
pub mod instance;
/// `jotra://` links: the grammar other programs point at a file with (ADR-052).
pub mod jotra_link;
/// The spellings from before the rename, read beside the current ones.
pub mod legacy_names;
/// External-link allowlist policy — ADR-025.
pub mod link;
/// The Linux autostart entry and AppImage files after the rename.
pub mod linux_rename;
/// Database bloat policy driving the startup vacuum.
pub mod maintenance;
/// The MCP clients Jotra connects, and what each is handed (ADR-031).
pub mod mcp_clients;
/// The names the MCP server answers under, by the name it was started as.
pub mod mcp_dialect;
/// The per-user record of the one-time steps the rename takes.
pub mod name_migration;
/// What of a note's past is kept, and what is dropped — spec H1.
pub mod note_history;
/// Notes-folder policy: root resolution, title sanitisation, dedupe — ADR-028.
pub mod notes;
/// The file open in Jotra, as a program allowed to see it is answered (ADR-053).
pub mod open_context;
/// Prompt construction and endpoint policy for opt-in text rewriting.
pub mod polish;
/// Preview surface types and content-type renderer registry — ADR-009.
pub mod preview;
/// Prompt-document helpers: token estimation, stripping, placeholders.
pub mod prompt;
/// Crash-recovery policy types and resolution logic.
pub mod recovery;
/// Full-text search query policy: prefix-match construction and sanitization.
pub mod search;
/// The seeded number stream the property tests draw their cases from.
#[cfg(test)]
pub(crate) mod seeded;
/// Work that runs one at a time per key, in the order it was asked for.
pub mod serial;
/// Startup-failure report formatting and report-location policy.
pub mod startup;
/// Which text encoding a file is in, and reading and writing it — ADR-049.
pub mod text_encoding;
/// What a double-click on the window's title bar does.
pub mod title_bar;
/// The tools a connected program can call, named once.
pub mod tools;
/// Update lifecycle phases and transition policy.
pub mod update;
/// The environment variables a person sets, read under both names.
pub mod user_variables;
/// External-change events and conflict-resolution policy.
pub mod watcher;
/// The Windows login item and app id after the rename.
pub mod windows_rename;
/// Workspace-level state (reserved for future expansion).
pub mod workspace;
/// Asking the person before a connected program's write lands (ADR-053).
pub mod write_review;
