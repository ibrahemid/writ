//! The protocol, and the only file in the crate that names `rmcp`.
//!
//! Every tool declared here forwards straight to a [`ToolHost`] method and
//! turns a [`ToolError`] into an MCP error. An SDK bump touches this file and
//! nothing else, which is why the tool surface next door carries no protocol
//! type (ADR-032 section 8).
//!
//! The transport is stdio, to a process the user's client launched. Nothing
//! here opens a socket or makes a request (ADR-031 rules 2.3 and 2.4).

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, Implementation, ServerCapabilities, ServerInfo};
use rmcp::{schemars, tool, tool_handler, tool_router};
use rmcp::{ErrorData, Peer, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;

use crate::consent::ClientId;
use crate::tools::{ToolError, ToolHost, MAX_RESULTS};

/// How many notes or hits a call answers with when the client named no number.
const DEFAULT_LIMIT: usize = 100;

/// What the server tells a client about itself.
const INSTRUCTIONS: &str = "Reads the notes in the user's Writ folder. \
     Paths are the ones list_notes returns. No tool here changes a note.";

/// Arguments to `list_notes`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListNotesArgs {
    /// Only notes whose path starts with this, either the whole path
    /// list_notes returns or the part of it inside the folder.
    pub prefix: Option<String>,
    /// Most notes to return. Up to 500.
    pub limit: Option<usize>,
}

/// Arguments to `search_notes`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchNotesArgs {
    /// The words to look for.
    pub query: String,
    /// Most hits to return. Up to 500.
    pub limit: Option<usize>,
}

/// Arguments to every tool that names one note.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct NotePathArgs {
    /// The note's path, as list_notes returns it.
    pub path: String,
}

/// The MCP server over one notes folder.
pub struct WritServer {
    host: ToolHost,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl WritServer {
    /// A server answering from `host`.
    pub fn new(host: ToolHost) -> Self {
        Self {
            host,
            tool_router: Self::tool_router(),
        }
    }

    /// The names this server registers.
    pub fn tool_names(&self) -> Vec<String> {
        self.tool_router
            .list_all()
            .into_iter()
            .map(|tool| tool.name.to_string())
            .collect()
    }

    #[tool(name = "list_notes", description = "List the notes in the folder.")]
    async fn list_notes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<ListNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(
            self.host
                .list_notes(&client_of(&peer), args.prefix.as_deref(), limit(args.limit)),
        )
    }

    #[tool(
        name = "search_notes",
        description = "Find notes whose text matches a query."
    )]
    async fn search_notes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<SearchNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(
            self.host
                .search_notes(&client_of(&peer), &args.query, limit(args.limit)),
        )
    }

    #[tool(
        name = "read_note",
        description = "Read one note, frontmatter included."
    )]
    async fn read_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(self.host.read_note(&client_of(&peer), &args.path))
    }

    #[tool(name = "note_links", description = "The links written in one note.")]
    async fn note_links(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(self.host.note_links(&client_of(&peer), &args.path))
    }

    #[tool(
        name = "note_backlinks",
        description = "The links in other notes that point at one note."
    )]
    async fn note_backlinks(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(self.host.note_backlinks(&client_of(&peer), &args.path))
    }

    #[tool(
        name = "note_properties",
        description = "The frontmatter properties of one note."
    )]
    async fn note_properties(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(self.host.note_properties(&client_of(&peer), &args.path))
    }

    #[tool(name = "note_tags", description = "The tags written in one note.")]
    async fn note_tags(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        answer(self.host.note_tags(&client_of(&peer), &args.path))
    }

    #[tool(
        name = "folder_tags",
        description = "Every tag in the folder, with the number of notes carrying each."
    )]
    async fn folder_tags(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, ErrorData> {
        answer(self.host.folder_tags(&client_of(&peer)))
    }
}

#[tool_handler]
impl ServerHandler for WritServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("writ", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}

/// Serves `host` on this process's stdin and stdout until the client closes it.
///
/// The client launched this process and owns its lifetime, so the future
/// finishes when the pipe does.
pub async fn serve_stdio(host: ToolHost) -> Result<(), ServeError> {
    serve_on(host, rmcp::transport::io::stdio()).await
}

/// Serves `host` over a transport the caller owns, until that transport closes.
///
/// [`serve_stdio`] is this over the process's own pipes. A test drives the same
/// server over a duplex pair, so the protocol is exercised without a process.
pub async fn serve_on<T, E, A>(host: ToolHost, transport: T) -> Result<(), ServeError>
where
    T: rmcp::transport::IntoTransport<RoleServer, E, A>,
    E: std::error::Error + Send + Sync + 'static,
{
    let running = WritServer::new(host)
        .serve(transport)
        .await
        .map_err(|error| ServeError::Initialize(error.to_string()))?;
    running
        .waiting()
        .await
        .map_err(|error| ServeError::Serving(error.to_string()))?;
    Ok(())
}

/// Why the server stopped before the client closed it.
///
/// Both messages carry the SDK's own text, which names transports and status
/// codes and no note content (ADR-031 rule 5.2).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ServeError {
    /// The handshake did not complete.
    #[error("the client connection could not be opened: {0}")]
    Initialize(String),
    /// The connection failed while it was being served.
    #[error("the client connection ended: {0}")]
    Serving(String),
}

/// The `clientInfo` the peer sent at initialize, or an empty name when it sent
/// none. An empty name is refused by every gate, so a peer that never
/// initialised reads no note.
fn client_of(peer: &Peer<RoleServer>) -> ClientId {
    match peer.peer_info() {
        Some(info) => ClientId {
            name: info.client_info.name.clone(),
            version: Some(info.client_info.version.clone()),
        },
        None => ClientId::default(),
    }
}

/// The limit a client asked for, held to [`MAX_RESULTS`].
fn limit(asked: Option<usize>) -> usize {
    asked.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_RESULTS)
}

/// One tool's answer as MCP content, or its refusal as an MCP error.
fn answer<T: serde::Serialize>(result: Result<T, ToolError>) -> Result<CallToolResult, ErrorData> {
    let value = result.map_err(mcp_error)?;
    let text = serde_json::to_string_pretty(&value)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    Ok(CallToolResult::success(vec![ContentBlock::text(text)]))
}

/// [`ToolError`] as the error code a client renders.
fn mcp_error(error: ToolError) -> ErrorData {
    let message = error.to_string();
    match error {
        ToolError::NotApproved { .. } => ErrorData::invalid_request(message, None),
        ToolError::OutsideNotesFolder { .. } | ToolError::TooLarge { .. } => {
            ErrorData::invalid_params(message, None)
        }
        ToolError::NotFound { .. } => ErrorData::resource_not_found(message, None),
        ToolError::IndexUnavailable | ToolError::Unreadable { .. } => {
            ErrorData::internal_error(message, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::{DenyAll, EnabledReads};
    use crate::tools::READ_TOOLS;

    fn host(root: &std::path::Path, enabled: bool) -> ToolHost {
        let db = root.join("writ.db");
        if enabled {
            ToolHost::open(root, &db, Box::new(EnabledReads::new(true))).expect("host")
        } else {
            ToolHost::open(root, &db, Box::new(DenyAll)).expect("host")
        }
    }

    #[test]
    fn the_registered_tools_are_the_read_tools_and_nothing_else() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let server = WritServer::new(host(dir.path(), true));

        let mut names = server.tool_names();
        names.sort();
        let mut expected: Vec<String> = READ_TOOLS.iter().map(|name| name.to_string()).collect();
        expected.sort();

        assert_eq!(names, expected);
    }

    #[test]
    fn no_write_tool_is_registered() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let server = WritServer::new(host(dir.path(), true));

        for name in ["write_note", "create_note", "rename_note", "trash_note"] {
            assert!(!server.tool_names().contains(&name.to_string()), "{name}");
        }
    }

    #[test]
    fn the_server_declares_tools_and_names_itself_writ() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let info = WritServer::new(host(dir.path(), false)).get_info();

        assert!(info.capabilities.tools.is_some());
        assert_eq!(info.server_info.name, "writ");
    }

    #[test]
    fn a_limit_a_client_asked_for_is_held_to_the_ceiling() {
        assert_eq!(limit(None), DEFAULT_LIMIT);
        assert_eq!(limit(Some(0)), 1);
        assert_eq!(limit(Some(10)), 10);
        assert_eq!(limit(Some(MAX_RESULTS * 4)), MAX_RESULTS);
    }

    #[test]
    fn a_refusal_carries_the_tool_errors_own_words() {
        let refused = mcp_error(ToolError::NotApproved {
            client: "Some Client".to_string(),
            tool: "read_note".to_string(),
        });
        assert!(refused.message.contains("Some Client"));

        let outside = mcp_error(ToolError::OutsideNotesFolder {
            path: "/etc/passwd".to_string(),
        });
        assert!(outside.message.contains("/etc/passwd"));
    }
}
