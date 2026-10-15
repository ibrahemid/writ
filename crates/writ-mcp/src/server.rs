//! The protocol, and the only file in the crate that names `rmcp`.
//!
//! Every tool declared here forwards straight to a [`ToolHost`] method and
//! turns a [`ToolError`] into an MCP error. An SDK bump touches this file and
//! nothing else, which is why the tool surface next door carries no protocol
//! type (ADR-032 section 8).
//!
//! Each tool body runs on the blocking pool: a walk of a large folder or an
//! index query holds a pool thread, not the one thread the protocol runs on,
//! so a cancellation or a ping is answered while it works. Every answer is a
//! JSON object, sent as structured content and as the same compact JSON in a
//! text block for a client that reads only text.
//!
//! The transport is stdio, to a process the user's client launched. Nothing
//! here opens a socket or makes a request (ADR-031 rules 2.3 and 2.4).

use std::sync::Arc;

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Implementation, ListResourceTemplatesResult, ListResourcesResult,
    PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse, ReadResourceResult,
    Resource, ResourceContents, ResourceTemplate, ServerCapabilities, ServerConfig,
};
use rmcp::service::RequestContext;
use rmcp::{schemars, tool, tool_handler, tool_router};
use rmcp::{ErrorData, Peer, RoleServer, ServerHandler, ServiceExt};
use serde::Deserialize;

use crate::consent::ClientId;
use crate::tools::{
    file_resource_uri, resource_target, ResourceTarget, ToolError, ToolHost, FILE_RESOURCE_PREFIX,
    INDEX_RESOURCE, MAX_RESULTS,
};

/// How many notes or hits a call answers with when the client named no number.
const DEFAULT_LIMIT: usize = 100;

/// What the server tells a client about itself.
const INSTRUCTIONS: &str = "Reads and writes the notes in the user's Writ folder. Paths are \
     relative to that folder, such as Ideas/Launch.md, in answers and in arguments. \
     list_notes and search_notes answer a page at a time: pass the next_cursor an answer \
     carries as cursor to get the next page. Reading and writing are approved separately, \
     in Writ. To change part of a note, use append_note to add to its end or \
     replace_in_note to swap one passage, rather than sending the whole note back. Read a \
     note before you write it whole: write_note compares the note with the text you read, \
     and writes a note you have not read only when you pass overwrite. today is the day's \
     file. open_in_writ shows a file to the user in Writ, and answers with open_url, a \
     writ:// link you can hand them. get_context answers the file open in Writ and what \
     the user selected in it, when the user lets this program see it. The folder index resource lists every file with its title, tags and \
     links. Writ keeps the text every write replaces. Nothing here deletes a note.";

/// Arguments to `list_notes`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListNotesArgs {
    /// Only notes whose path starts with this, such as Projects/.
    pub prefix: Option<String>,
    /// The next_cursor an earlier list_notes answer carried, for the page
    /// after it.
    pub cursor: Option<String>,
    /// Most notes to return. Up to 500.
    pub limit: Option<usize>,
}

/// Arguments to `search_notes`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchNotesArgs {
    /// The words to look for.
    pub query: String,
    /// The next_cursor an earlier search_notes answer carried, for the page
    /// after it.
    pub cursor: Option<String>,
    /// Most hits to return. Up to 500.
    pub limit: Option<usize>,
}

/// Arguments to every tool that names one note.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct NotePathArgs {
    /// The note's path inside the notes folder, as list_notes returns it,
    /// such as Ideas/Launch.md.
    pub path: String,
}

/// Arguments to `write_note`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct WriteNoteArgs {
    /// The note's path inside the notes folder, as list_notes returns it,
    /// such as Ideas/Launch.md. The note has to be there already;
    /// create_note is what makes a new one.
    pub path: String,
    /// The whole note, frontmatter included. It replaces what the file
    /// holds, byte for byte.
    pub content: String,
    /// The hash read_note or an earlier write_note returned for this note.
    /// The write is made only while the note still holds that text; a note
    /// somebody edited in between is left alone and your text is written
    /// beside it. Leave it out and the note is compared with the text you
    /// last read or wrote in this session, and a note you have not read is
    /// refused.
    pub expected_hash: Option<String>,
    /// Replace whatever the note holds now without comparing it with
    /// anything you read. The only way to write a note you have not read in
    /// this session; not allowed with expected_hash. Writ keeps the text it
    /// replaces and its activity list names the overwrite.
    #[serde(default)]
    pub overwrite: bool,
}

/// Arguments to `create_note`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct CreateNoteArgs {
    /// What to call the note. It becomes the file name, in the notes
    /// folder. A name ending in .md, .markdown, .txt or .text keeps that
    /// format; any other name gets the format set in Writ. A name already
    /// taken is not written over.
    pub name: String,
    /// The text the note starts with.
    pub content: String,
}

/// Arguments to `rename_note`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct RenameNoteArgs {
    /// The note's path inside the notes folder, as list_notes returns it.
    pub path: String,
    /// What to call it instead. It stays in the folder it is in.
    pub new_name: String,
}

/// Arguments to `append_note`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct AppendNoteArgs {
    /// The note's path inside the notes folder, as list_notes returns it.
    pub path: String,
    /// The text to add. It goes on lines of its own after what the note
    /// holds, in the note's line ending.
    pub text: String,
}

/// Arguments to `replace_in_note`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ReplaceInNoteArgs {
    /// The note's path inside the notes folder, as list_notes returns it.
    pub path: String,
    /// The exact text to replace, as the note holds it. It has to be in the
    /// note once: include enough of the text around it to be sure.
    pub old_text: String,
    /// The text to put in its place. Empty removes the passage.
    pub new_text: String,
}

/// Arguments to `today`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct TodayArgs {
    /// Text to add to the end of today's file. Leave it out to get the file
    /// without changing it.
    pub text: Option<String>,
}

/// Arguments to `open_in_writ`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct OpenInWritArgs {
    /// The file's path inside the notes folder, as list_notes returns it.
    pub path: String,
    /// The 1-based line to show.
    pub line: Option<u32>,
}

/// Arguments to `reveal`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct RevealArgs {
    /// The file's path inside the notes folder, as list_notes or get_context
    /// returns it.
    pub path: String,
    /// The heading to show, as the file writes it, with or without its #
    /// marks. Not with line.
    pub heading: Option<String>,
    /// The 1-based line to show. Not with heading.
    pub line: Option<u32>,
}

/// The MCP server over one notes folder.
pub struct WritServer {
    host: Arc<ToolHost>,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl WritServer {
    /// A server answering from `host`.
    pub fn new(host: ToolHost) -> Self {
        Self {
            host: Arc::new(host),
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

    /// Runs `body` against the host on the blocking pool, as `peer`'s call.
    async fn on_host<T, F>(
        &self,
        peer: &Peer<RoleServer>,
        body: F,
    ) -> Result<CallToolResult, ErrorData>
    where
        T: serde::Serialize + Send + 'static,
        F: FnOnce(&ToolHost, &ClientId) -> Result<T, ToolError> + Send + 'static,
    {
        let host = Arc::clone(&self.host);
        let client = client_of(peer);
        let answered = tokio::task::spawn_blocking(move || body(&host, &client))
            .await
            .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        answer(answered)
    }

    #[tool(
        name = "list_notes",
        description = "List the notes in the folder, ordered by path, a page at a time.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn list_notes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<ListNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.list_notes(
                client,
                args.prefix.as_deref(),
                args.cursor.as_deref(),
                limit(args.limit),
            )
        })
        .await
    }

    #[tool(
        name = "search_notes",
        description = "Find notes whose text matches a query, best match first, a page at a time.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn search_notes(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<SearchNotesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.search_notes(
                client,
                &args.query,
                args.cursor.as_deref(),
                limit(args.limit),
            )
        })
        .await
    }

    #[tool(
        name = "read_note",
        description = "Read one note, frontmatter included.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn read_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.read_note(client, &args.path)
        })
        .await
    }

    #[tool(
        name = "note_links",
        description = "The links written in one note.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn note_links(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.note_links(client, &args.path)
                .map(|links| serde_json::json!({ "links": links }))
        })
        .await
    }

    #[tool(
        name = "note_backlinks",
        description = "The links in other notes that point at one note.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn note_backlinks(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.note_backlinks(client, &args.path)
                .map(|backlinks| serde_json::json!({ "backlinks": backlinks }))
        })
        .await
    }

    #[tool(
        name = "note_properties",
        description = "The frontmatter properties of one note.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn note_properties(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.note_properties(client, &args.path)
                .map(|properties| serde_json::json!({ "properties": properties }))
        })
        .await
    }

    #[tool(
        name = "note_tags",
        description = "The tags written in one note.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn note_tags(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<NotePathArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.note_tags(client, &args.path)
                .map(|tags| serde_json::json!({ "tags": tags }))
        })
        .await
    }

    #[tool(
        name = "folder_tags",
        description = "Every tag in the folder, with the number of notes carrying each.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn folder_tags(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.folder_tags(client)
                .map(|tags| serde_json::json!({ "tags": tags }))
        })
        .await
    }

    #[tool(
        name = "write_note",
        description = "Replace the text of a note that already exists. Read it first: a note \
            edited since you read it is left alone, with your text written beside it, and a \
            note you have not read is refused unless you pass overwrite. Writ keeps the text \
            every write replaces.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn write_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<WriteNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.write_note(
                client,
                &args.path,
                &args.content,
                args.expected_hash.as_deref(),
                args.overwrite,
            )
        })
        .await
    }

    #[tool(
        name = "create_note",
        description = "Make a new note in the folder. A name ending in .md, .markdown, .txt \
            or .text is created in that format, and any other name in the format set in \
            Writ. A name the folder already holds is not written over.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn create_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<CreateNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.create_note(client, &args.name, &args.content)
        })
        .await
    }

    #[tool(
        name = "rename_note",
        description = "Rename a note, inside the folder it is in. Links in other notes are \
            not updated: they keep pointing at the old name.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn rename_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<RenameNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.rename_note(client, &args.path, &args.new_name)
        })
        .await
    }

    #[tool(
        name = "append_note",
        description = "Add text to the end of a note that already exists, on lines of its \
            own, without sending the note back. Writ keeps the text the note held. Answers \
            with open_url, a writ:// link that shows the note in Writ.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn append_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<AppendNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.append_note(client, &args.path, &args.text)
        })
        .await
    }

    #[tool(
        name = "replace_in_note",
        description = "Replace one passage of a note: old_text, exactly as the note holds \
            it and found once, becomes new_text. Text the note does not hold, or holds more \
            than once, is refused and nothing is written. Writ keeps the text the note held. \
            Answers with the line the new text starts on and open_url.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn replace_in_note(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<ReplaceInNoteArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.replace_in_note(client, &args.path, &args.old_text, &args.new_text)
        })
        .await
    }

    #[tool(
        name = "today",
        description = "Today's file, the one Today's File opens in Writ. It is made when the \
            day has none, and text you pass is added to its end. Answers with its path and \
            open_url.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn today(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<TodayArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.today(client, args.text.as_deref())
        })
        .await
    }

    #[tool(
        name = "open_in_writ",
        description = "Show a file to the user in Writ, at a line when you pass one. Changes \
            no file. Answers with open_url, the writ:// link Writ was handed.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn open_in_writ(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<OpenInWritArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.open_in_writ(client, &args.path, args.line)
        })
        .await
    }

    #[tool(
        name = "reveal",
        description = "Show a file to the user in Writ at a heading or a line. Changes no \
            file. Answers with open_url, the writ:// link Writ was handed.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn reveal(
        &self,
        peer: Peer<RoleServer>,
        Parameters(args): Parameters<RevealArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| {
            host.reveal(client, &args.path, args.heading.as_deref(), args.line)
        })
        .await
    }

    #[tool(
        name = "get_context",
        description = "The file the user has open in Writ and the text they selected in \
            it, with the line and column each selection starts and ends at, and whether \
            the tab has unsaved text. Answers only for a program the user lets see the \
            open file. Without a file open, path is absent.",
        annotations(read_only_hint = true, open_world_hint = false)
    )]
    async fn get_context(&self, peer: Peer<RoleServer>) -> Result<CallToolResult, ErrorData> {
        self.on_host(&peer, move |host, client| host.get_context(client))
            .await
    }
}

#[tool_handler]
impl ServerHandler for WritServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .build(),
        )
        .with_server_info(Implementation::new("writ", env!("CARGO_PKG_VERSION")))
        .with_instructions(INSTRUCTIONS)
    }

    /// The folder index first, then the files a page at a time, approved and
    /// logged as `list_notes` is.
    async fn list_resources(
        &self,
        request: Option<PaginatedRequestParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, ErrorData> {
        let host = Arc::clone(&self.host);
        let client = client_of(&context.peer);
        let cursor = request.and_then(|request| request.cursor);
        let listed = tokio::task::spawn_blocking(move || {
            host.list_notes(&client, None, cursor.as_deref(), DEFAULT_LIMIT)
                .map(|page| (page, cursor.is_none()))
        })
        .await
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let (page, first) = listed.map_err(mcp_error)?;
        let mut resources = Vec::with_capacity(page.notes.len() + 1);
        if first {
            resources.push(
                Resource::new(INDEX_RESOURCE, "Folder index")
                    .with_description(
                        "Every file in the folder with its title, tags, the files it links \
                         to and its size, read from Writ's index.",
                    )
                    .with_mime_type("application/json"),
            );
        }
        for note in page.notes {
            resources.push(
                Resource::new(file_resource_uri(&note.path), note.path.clone())
                    .with_title(note.name)
                    .with_mime_type(mime_for(&note.path))
                    .with_size(note.bytes),
            );
        }
        let mut result = ListResourcesResult::with_all_items(resources);
        result.next_cursor = page.next_cursor;
        Ok(result)
    }

    /// One template: a file by its path.
    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, ErrorData> {
        Ok(ListResourceTemplatesResult::with_all_items(vec![
            ResourceTemplate::new(format!("{FILE_RESOURCE_PREFIX}{{+path}}"), "File")
                .with_description("A file in the folder, by its path, such as Ideas/Launch.md."),
        ]))
    }

    /// A file, read as `read_note` reads it, or the folder index, rendered
    /// from `writ.db` now. Both go through the gate.
    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, ErrorData> {
        let uri = request.uri;
        let Some(target) = resource_target(&uri) else {
            return Err(mcp_error(ToolError::UnknownResource { uri }));
        };
        let host = Arc::clone(&self.host);
        let client = client_of(&context.peer);
        let read = tokio::task::spawn_blocking(move || match target {
            ResourceTarget::Index => host.folder_index(&client).and_then(|index| {
                let text =
                    serde_json::to_string(&index).map_err(|_| ToolError::IndexUnavailable)?;
                Ok((text, "application/json".to_string()))
            }),
            ResourceTarget::File(path) => host
                .read_note(&client, &path)
                .map(|note| (note.text, mime_for(&path).to_string())),
        })
        .await
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
        let (text, mime) = read.map_err(mcp_error)?;
        Ok(
            ReadResourceResult::new(vec![ResourceContents::text(text, uri).with_mime_type(mime)])
                .into(),
        )
    }
}

/// The media type a file resource is read as: Markdown for a Markdown name,
/// plain text for every other text file.
fn mime_for(path: &str) -> &'static str {
    if writ_core::notes::extensions::has_markdown_extension(std::path::Path::new(path)) {
        "text/markdown"
    } else {
        "text/plain"
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
///
/// Structured content, with the same JSON compact in a text block beside it:
/// no indentation and no second rendering for a model to read through.
fn answer<T: serde::Serialize>(result: Result<T, ToolError>) -> Result<CallToolResult, ErrorData> {
    let value = result.map_err(mcp_error)?;
    let value = serde_json::to_value(&value)
        .map_err(|error| ErrorData::internal_error(error.to_string(), None))?;
    Ok(CallToolResult::structured(value))
}

/// [`ToolError`] as the error code a client renders.
fn mcp_error(error: ToolError) -> ErrorData {
    let message = error.to_string();
    match error {
        ToolError::NotEnabled
        | ToolError::NotApproved { .. }
        | ToolError::NotAllowed { .. }
        | ToolError::ContextNotAllowed { .. }
        | ToolError::Unnamed => ErrorData::invalid_request(message, None),
        ToolError::OutsideNotesFolder { .. }
        | ToolError::TooLarge { .. }
        | ToolError::TooMuchText { .. }
        | ToolError::HashNotUnderstood { .. }
        | ToolError::CursorNotUnderstood
        | ToolError::HashAndOverwrite { .. }
        | ToolError::NameEmpty
        | ToolError::NameTaken { .. } => ErrorData::invalid_params(message, None),
        ToolError::NotFound { .. } | ToolError::UnknownResource { .. } => {
            ErrorData::resource_not_found(message, None)
        }
        ToolError::PassageEmpty { .. }
        | ToolError::PassageAbsent { .. }
        | ToolError::PassageRepeated { .. }
        | ToolError::NotInAppFolder { .. }
        | ToolError::HeadingAbsent { .. }
        | ToolError::HeadingAndLine { .. } => ErrorData::invalid_params(message, None),
        ToolError::WritUnreachable { .. }
        | ToolError::ContextUnanswered
        | ToolError::ReviewUnavailable { .. } => ErrorData::internal_error(message, None),
        // The person's answer to the call, as a conflict is the note's.
        ToolError::Discarded { .. } => ErrorData::invalid_request(message, None),
        // A conflict is the note's answer to the call, not a fault in it: the
        // client asked about a note it had read and the note has moved on.
        ToolError::Conflict { .. } => ErrorData::invalid_request(message, None),
        // The same for a note the client has not read: the call is well formed,
        // and the client has a read to make before it is answered.
        ToolError::HashRequired { .. } => ErrorData::invalid_request(message, None),
        ToolError::IndexUnavailable
        | ToolError::NotesFolderMissing
        | ToolError::VersionsUnavailable { .. }
        | ToolError::Unreadable { .. }
        | ToolError::Unwritable { .. }
        | ToolError::NotDownloaded { .. } => ErrorData::internal_error(message, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consent::{DenyAll, EnabledReads};
    use crate::tools::{FixedAppFolder, CONTEXT_TOOLS, READ_TOOLS, WRITE_TOOLS};

    fn host(root: &std::path::Path, enabled: bool) -> ToolHost {
        let db = root.join("writ.db");
        if enabled {
            ToolHost::open(
                root,
                &db,
                root,
                Box::new(FixedAppFolder(root.to_path_buf())),
                Box::new(EnabledReads::new(true)),
            )
            .expect("host")
        } else {
            ToolHost::open(
                root,
                &db,
                root,
                Box::new(FixedAppFolder(root.to_path_buf())),
                Box::new(DenyAll),
            )
            .expect("host")
        }
    }

    #[test]
    fn the_registered_tools_are_the_ones_writ_core_names_and_nothing_else() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let server = WritServer::new(host(dir.path(), true));

        let mut names = server.tool_names();
        names.sort();
        let mut expected: Vec<String> = READ_TOOLS
            .iter()
            .chain(WRITE_TOOLS.iter())
            .chain(CONTEXT_TOOLS.iter())
            .map(|name| name.to_string())
            .collect();
        expected.sort();

        assert_eq!(
            names, expected,
            "the settings row is generated from writ_core::tools, so it has to be this set"
        );
    }

    #[test]
    fn the_features_page_names_every_tool_and_counts_them_as_the_server_does() {
        let features = include_str!("../../../docs/FEATURES.md");
        let line = features
            .lines()
            .find(|line| line.starts_with("- **Connected programs**"))
            .expect("the Connected programs line");
        for name in READ_TOOLS
            .iter()
            .chain(WRITE_TOOLS.iter())
            .chain(CONTEXT_TOOLS.iter())
        {
            assert!(line.contains(name), "FEATURES.md does not name {name}");
        }
        let words = [
            "Zero",
            "One",
            "Two",
            "Three",
            "Four",
            "Five",
            "Six",
            "Seven",
            "Eight",
            "Nine",
            "Ten",
            "Eleven",
            "Twelve",
            "Thirteen",
            "Fourteen",
            "Fifteen",
            "Sixteen",
            "Seventeen",
            "Eighteen",
            "Nineteen",
            "Twenty",
        ];
        let dir = tempfile::TempDir::new().expect("temp dir");
        let count = WritServer::new(host(dir.path(), true)).tool_names().len();
        let reads = words[READ_TOOLS.len()].to_lowercase();
        let writes = words[WRITE_TOOLS.len()].to_lowercase();
        let sees = words[CONTEXT_TOOLS.len()].to_lowercase();
        assert!(line.contains(&format!("{} tools", words[count])), "{line}");
        assert!(line.contains(&format!("{reads} that read")), "{line}");
        assert!(line.contains(&format!("{writes} that write")), "{line}");
        assert!(
            line.contains(&format!("{sees} that sees the file open in Writ")),
            "{line}"
        );
    }

    #[test]
    fn no_tool_deletes_trashes_or_moves_a_note() {
        let dir = tempfile::TempDir::new().expect("temp dir");
        let server = WritServer::new(host(dir.path(), true));

        for name in [
            "delete_note",
            "trash_note",
            "move_note",
            "remove_note",
            "note_delete",
        ] {
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

        let off = mcp_error(ToolError::NotEnabled);
        assert_eq!(
            off.message,
            "Connected programs is off in Writ. Turn it on in Settings, Apps."
        );
        assert_eq!(
            off.code, refused.code,
            "every refusal is the same kind of answer"
        );

        let outside = mcp_error(ToolError::OutsideNotesFolder {
            path: "/etc/passwd".to_string(),
        });
        assert!(outside.message.contains("/etc/passwd"));
    }
}
