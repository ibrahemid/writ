# ADR-040: One AI connection, and a chat that persists

## Status

Proposed, 2026-09-14. Lands in front of the 1.0 tag.

Amends [ADR-031](./031-the-ai-harness-and-what-leaves-the-machine.md) in four places (rules 2.1,
2.4, 5.2 and 7.1, stated in section 10). Stands on [ADR-028](./028-files-are-the-only-copy.md),
which keeps everything that is not a note out of the notes folder, and on
[ADR-032](./032-the-internal-surface-notes-are-reached-through.md), whose permission sets the chat
pane keeps. ADR-027 section 2 put the AI section in Settings; this record renames it and gives it
one shape.

Three units build it: U1 connection and settings, U2 chat pane, U3 site, docs, capture and
changelog. Where a rule below names a unit, that unit is reviewed against it.

## Context

The assessment of 2026-09-14 (`.status/reports/ai-surfaces-assessment-2026-09-14.md`) found a
configuration surface that asks a non-developer for a base URL, a model id and an API key twice,
under two credential sets, with no help text on any of ten rows, inside a section named "AI
rewriting" that also holds the chat. The rewrite path stores its key under the bare preset id
(`src-tauri/src/commands/ai.rs:232-262`); the chat path stores its own under `chat:<provider>`
(`crates/writ-core/src/chat.rs:73-78`), so a person who pasted a Groq key for rewriting pastes it
again for chat, while host consent is already shared between the two
(`crates/writ-core/src/config/ai.rs:47-53`).

The chat pane streams, stops, attaches open notes and applies a proposal through the guarded write.
It also prints the raw `writ-proposal` fence and the whole proposed note above the card
(`src-tauri/src/commands/chat.rs:814-816` emits every delta verbatim; `parse_proposals` at
`crates/writ-core/src/chat.rs:393` removes nothing), renders replies as one `pre-wrap` paragraph,
shows nothing between Send and the first token, offers no retry, and keeps the conversation in a
`createRoot` signal that dies with the process (`src/stores/global/chat.ts:57`).

The benchmark (`.status/reports/ai-chat-benchmark-2026-09-14.md`, sections B and D) puts the bar:
a provider dropdown with local runtimes detected, a fetched model list, a key field with a link to
get one, a check line, and a pane with rendered Markdown, persistence, edit-and-resend, `@` to
attach, and a model picker in the composer. The feasibility report
(`.status/reports/ai-subscription-feasibility-2026-09-14.md`) settles which credentials Writ may
use: ten hosted providers by key, OpenRouter by PKCE, and none of the CLI logins.

## Decision drivers

- A non-developer picks a provider and is chatting inside a minute, having typed at most one key.
- One key, one host consent, one model list serve both features. Nothing is configured twice.
- ADR-031's privacy claim stays one checkable sentence: the destinations are the host the user
  chose and the updater, and nothing reaches a hosted host before Allow.
- No credential another program stored is read, ever.
- A conversation survives relaunch, lives outside the notes folder, and holds no note text.
- Every new shape upgrades an existing `config.toml` and an existing keychain without a re-paste.

## Considered options

### The connection model

1. **Two connections, shared key.** Keep `[ai]` and `[ai.chat]` as they are and make the chat rows
   read the rewrite account when their own is empty. Cheapest, and it leaves ten rows and two base
   URLs on the screen. Rejected: the screen is the defect.
2. **One connection, one model, no override.** One provider, key and model for both features.
   Simplest screen. Rejected only because the model a person talks to is often not the small one
   that proofreads a sentence, and the assessment's own config comment says so.
3. **One connection, optional chat model override.** Chosen. Provider, key and consent are one;
   the model is one by default; a disclosure reveals one extra model dropdown for chat and nothing
   else.

### Where conversations live

1. **A `conversations` table in `writ.db`.** One database, one migration path. Rejected: ADR-028
   makes `writ.db` derived data that can be deleted at the cost of a reindex, and a conversation is
   not derivable from anything.
2. **A `chats.db` beside `history.db`.** The precedent `note_history` set for content that is not
   a note (`crates/writ-storage/src/note_history/mod.rs:9`). Sound, and more machinery than the
   access pattern needs: the pane lists by recency, opens one, and writes one.
3. **One JSON file per conversation under the data directory.** Chosen. A conversation is a
   document with a single writer; an atomic rename makes each save crash-safe; listing is a
   directory read; a person can back one up, read it, or delete it without a tool.

### Bringing a subscription

1. **Reuse Claude Code, Codex CLI or Gemini CLI credentials.** Refused; section 6 says why.
2. **API key per provider plus OpenRouter PKCE.** Chosen for this record.
3. **GitHub Copilot through the Copilot SDK.** Permitted and documented, and it is an agent runtime
   over a second sidecar binary with no plain chat mode. Deferred to a later record (section 11).

## Decision

### 1. One connection, two features

`[ai]` becomes one connection and two feature switches:

```toml
[ai]
provider = "ollama"          # an id from the table in section 2
base_url = ""                # read only when provider = "custom"
model = ""                   # empty until the list is fetched or the user picks
consented_hosts = []         # unchanged, ADR-031 rule 6.1

[ai.rewrite]
enabled = false

[ai.chat]
enabled = false
model = ""                   # empty means the connection's model
```

`crates/writ-core/src/config/ai.rs` deserialises through an on-disk shape that carries the old
fields and the new ones, every one defaulted, and converts with one pure function,
`AiConfig::from(AiConfigOnDisk)`. That function is the migration and it is tested once per old
shape:

| Old shape | Result |
|---|---|
| Pre-chat: `enabled`, `preset`, `base_url`, `model` | `provider = preset`, `rewrite.enabled = enabled`, `model` kept; `base_url` kept only when `preset = "custom"` |
| Rewrite on, `[chat]` off or absent | as above; `chat.enabled = false` |
| Rewrite off, `[chat]` on with `provider = "anthropic"` | `provider = "anthropic"`, `model = chat.model`, `chat.enabled = true` |
| Rewrite off, `[chat]` on with `openai_compatible` and a `base_url` equal to a preset's | that preset, `model = chat.model` |
| Rewrite off, `[chat]` on with `openai_compatible` and an unknown `base_url` | `provider = "custom"`, `base_url` kept, `model = chat.model` |
| Both on, same provider | connection from the rewrite side; `chat.model = chat.model` when it differs |
| Both on, different providers | connection from the rewrite side; `chat.model` empty; `chat.enabled` kept. The chat picks up the connection's model on its next send, which is the one case where a person makes one choice again, and the send dialog names it |
| Empty table | defaults, all off, provider `ollama` |

Serialisation writes the new shape only, so a file upgrades on its next save. `consented_hosts`
carries over untouched in every row. The 0.5 tests that pin the old shape
(`crates/writ-core/src/config/ai.rs:139-217`) are rewritten to pin these rows.

The settings section is named "AI" (`src/settings/index.ts`). It shows one connection block
(Provider, API key, Model, connection line with Check) and then two groups, "Rewrite selected
text" and "Chat about your notes". The five chat rows go away. The disclosure "Use a different
model for chat" reveals one model dropdown bound to `ai.chat.model`. `SettingsRow` gains a
`description` prop and every row in the section carries one; the strings are the benchmark's
section D copy and pass the copy gate. The pinned labels in
`src/__tests__/architecture/settings-vocabulary.test.ts` and `SettingsModal.test.tsx` change to the
new names on purpose (U1).

### 2. The provider table lives in `writ-core`

Today the presets are a frontend map (`SettingsModal.tsx:828-836`, `ai-models.ts:6-13`) and the
Rust side knows nothing but a base URL. The table moves to `crates/writ-core/src/ai/providers.rs`
as data, and one command, `ai_providers`, serialises it, so the dropdown, the probe, the model
fetch and the endpoint guard read one definition.

| id | Label | Group | Wire | Base URL | Models | Key |
|---|---|---|---|---|---|---|
| `ollama` | Ollama | local | OpenAI | `http://localhost:11434/v1` | `GET http://localhost:11434/api/tags` | none |
| `lmstudio` | LM Studio | local | OpenAI | `http://localhost:1234/v1` | `GET http://localhost:1234/v1/models` | none |
| `anthropic` | Anthropic | hosted | Anthropic | `https://api.anthropic.com` | `GET /v1/models` with `x-api-key` and `anthropic-version` | key |
| `openai` | OpenAI | hosted | OpenAI | `https://api.openai.com/v1` | `GET /v1/models` | key |
| `gemini` | Google Gemini | hosted | OpenAI | `https://generativelanguage.googleapis.com/v1beta/openai` | `GET https://generativelanguage.googleapis.com/v1beta/models?key=` | key |
| `openrouter` | OpenRouter | hosted | OpenAI | `https://openrouter.ai/api/v1` | `GET /api/v1/models`, public | key or Connect |
| `groq` | Groq | hosted | OpenAI | `https://api.groq.com/openai/v1` | `GET /openai/v1/models` | key |
| `deepseek` | DeepSeek | hosted | OpenAI | `https://api.deepseek.com` | `GET /models` | key |
| `mistral` | Mistral | hosted | OpenAI | `https://api.mistral.ai/v1` | `GET /v1/models` | key |
| `xai` | xAI | hosted | OpenAI | `https://api.x.ai/v1` | `GET /v1/models` | key |
| `together` | Together | hosted | OpenAI | `https://api.together.xyz/v1` | `GET /v1/models` | key |
| `fireworks` | Fireworks | hosted | OpenAI | `https://api.fireworks.ai/inference/v1` | `GET /inference/v1/models` | key |
| `custom` | Custom (OpenAI-compatible) | custom | OpenAI | typed | `GET {base_url}/models` | optional |

One row departs from the feasibility table on purpose. Gemini's native root is `/v1beta`, and that
is where the model list is, but `chat/completions` is served under `/v1beta/openai`, which is what
the shipped preset already uses. The table therefore carries both: the OpenAI-compatible base for
requests and the native list URL, filtered on `supportedGenerationMethods` containing
`generateContent`. DeepSeek drops the `/v1` the old preset carried, matching the table; the server
answers both.

Each hosted row also carries the URL of its key page for the "Get a key" link and a default model
id. The defaults are the first curated entries in `ai-models.ts` today, with Anthropic's default
moved to `claude-sonnet-5`, and U1 verifies each id against the live list before it ships. The
local rows default to the first installed model, which the list answers. Every id in the table is
a keychain account name (section 5) and a `provider` value in `config.toml` (section 1).

Anthropic keeps `Provider::Anthropic` (`crates/writ-core/src/chat.rs:38`); every other row is
`Provider::OpenAiCompatible`. The rewrite path, which speaks only `chat/completions` today,
learns the Anthropic wire in U1 so the one connection serves both features on every row.
`polish::is_endpoint_allowed` (`crates/writ-core/src/polish.rs`) still runs on every request,
including against a hand-edited `custom` base URL.

### 3. The model list is fetched, and its failures are typed

`ai_list_models` runs against the configured provider with the user's key, one implementation per
wire family: OpenAI-shaped `{ data: [{ id }] }`, Anthropic's `{ data: [{ id, display_name }] }`
with cursor pagination followed to the end, Gemini's `{ models: [{ name, supportedGenerationMethods
}] }`, Ollama's `{ models: [{ name }] }`. Timeout is 5 seconds. The result is
`Result<Vec<ModelId>, ModelListError>` with variants `Unreachable`, `Timeout`, `Unauthorized`,
`Status(u16)`, `Malformed`, and the frontend maps each to a `connectionDisplay` string rather than
showing the error text (ADR-031 rule 5.3). On any error the dropdown falls back to the curated
ids, marked as such, and "Custom…" stays last.

OpenAI's list is filtered client-side to ids that answer `chat/completions`, by excluding the
`embedding`, `tts`, `whisper`, `dall-e`, `moderation` and `realtime` families by prefix; the rest
is sorted by id. Without the filter the dropdown is unusable, which the feasibility report's
section 6 records.

The list request carries the key, so it is a send under ADR-031 rule 2.2 and waits for Allow
(section 7). OpenRouter's list is public and is the one list fetched before a key exists; it still
waits for Allow, because the rule a person can verify is that nothing reaches a hosted host before
they press it.

### 4. The localhost probe is keyless, and it is not a destination

Selecting the provider dropdown, and opening the AI section, probes `GET
http://localhost:11434/api/tags` and `GET http://localhost:1234/v1/models` with a 1-second
timeout. Whichever answers shows a "Running" pill; the other shows "Not running" with the
benchmark's one-line help. The probe carries no credential, no note text and no header beyond what
the HTTP client adds to every request (host, accept, an empty user agent). Ollama's native
`/api/tags` is used for detection because it lists installed models without
a key, and the client is configured against `/v1` on both runtimes, which is the OpenAI-compatible
path.

ADR-031 rule 2.1 enumerates destinations, and the two probes reach addresses the user did not
configure. They are still not a third destination, for a reason the rule's own heading states:
"Nothing leaves the machine unless the user's chosen client sends it." A loopback request leaves
nothing. Rather than leave that to inference, this record adds rule 2.7 to ADR-031 (section 10):
a request to a loopback address that carries no credential and no note text is not a destination,
and the two probes are the only such requests. A probe that carried a key would be rule 2.2's
send.

### 5. One keychain account per provider, migrated on first read

The account is the provider id from section 2, under the existing service `com.writ.ai`
(`src-tauri/src/commands/ai.rs:103`). Rewrite and chat read the same account. The old accounts
are the bare preset ids (`groq`, `gemini`, `deepseek`, `openrouter`, `custom`, which already equal
the new ids and need no move) and the chat namespace (`chat:anthropic`, `chat:openai_compatible`).

Migration is lazy and per account, because every keychain read on macOS can raise a prompt
(`ai.rs:67-77`) and a person who never used AI should see none. When a key is requested for
provider `p` and the account `p` is empty, the resolver checks the legacy account for `p`, copies
the key into `p`, and deletes the legacy entry. The legacy account for `anthropic` is
`chat:anthropic`; for the provider the config migration derived from an `openai_compatible` chat
(section 1), it is `chat:openai_compatible`. A key present under both the new and a legacy
account keeps the new one. Nothing about a key is logged, before or after.

`src-tauri/src/commands/ai.rs` takes the keychain behind a `KeyStore` trait so the migration is
tested against an in-memory store, one test per legacy account name, plus one for the
both-present case and one for the never-had-a-key case that proves the legacy read is not made
when the new account answers. The session cache (`AiState.keychain_cache`) is keyed by the new
account.

### 6. OpenRouter connects by PKCE; the CLI logins are refused

OpenRouter is the one provider with a "Connect" button, implementing the flow at
`openrouter.ai/docs/use-cases/oauth-pkce`:

1. Writ generates a 32-byte verifier and a random state, derives the S256 challenge, and binds a
   listener to `127.0.0.1` on an ephemeral port.
2. It opens `https://openrouter.ai/auth?callback_url=http://127.0.0.1:<port>/callback&code_challenge=…&code_challenge_method=S256&state=…`
   in the user's browser through `tauri-plugin-opener`, which is the user's browser navigating,
   not a Writ request.
3. The listener accepts one request, checks `state`, answers a static page that says the window
   can be closed, and closes the socket.
4. Writ posts the code and the verifier to `https://openrouter.ai/api/v1/auth/keys` and stores the
   returned key under the `openrouter` account. The exchange is a request to the configured host,
   so it waits for Allow like any other (section 7).

The flow times out after five minutes, a second Connect cancels the first, a mismatched state is
refused and reported as a failure without detail, and neither the verifier, the code nor the key
reaches a log line or an error string. The listener exists only for the duration of one flow the
user started; ADR-031 rule 2.4 is amended to say so (section 10), and the threat-model check
becomes that no port is open at rest and none after a completed or abandoned Connect.

The Claude Code, Codex CLI and Gemini CLI credentials on a user's machine are not read, not
offered, and not mentioned in copy or docs. The reasons are the feasibility report's, restated so
this record stands alone:

- Anthropic prohibits it in writing, names the Agent SDK, bans collecting or intermediating the
  credential at all, and enforces server-side since January 2026.
- OpenAI has never permitted it, has declined to answer when asked, and its Terms of Use bar
  programmatic use of the Services. Working today is one server change from not working.
- Google stopped serving individual accounts through Gemini CLI on 2026-06-18, and the quota it
  carried rode Google's own OAuth client and a Code Assist licence a third party cannot hold.

There is also a Writ-side reason that outlives any vendor's terms: a key another program stored is
that program's consent, given to it, and reading it would be Writ sending a person's notes on a
credential they never handed to Writ.

### 7. Consent does not change shape

ADR-031 rules 2.2, 2.5, 2.6, 4.3, 4.4 and section 6 stand as written. The consent sentence is
generalised to cover both features, in the benchmark's wording: "The notes you attach and the text
you rewrite are sent to {host} with your API key. Writ also sends the key on its own to check the
host is reachable; nothing else leaves your machine." It appears under the provider row for a
hosted provider not yet allowed, with one Allow button, and the local rows show "Requests go to
{host} on this machine. Nothing leaves it." instead.

Every request to a hosted host waits for Allow: the model list, the check, the OpenRouter
exchange, a rewrite and a chat. The send dialog before a chat still names the host and the bytes,
attached notes are still the only note text sent, and a proposal still applies through
`write_note_guarded` with `before_hash` (ADR-032 section 4). The context builder keeps
`{ReadNote}` and the applier keeps `{WriteNote}`.

### 8. Conversations are files under the data directory

A conversation is one JSON file at `<writ_dir>/chats/<uuid>.json`, where `<writ_dir>` is
`~/.writ` or `WRIT_DATA_DIR` (`src-tauri/src/state.rs:1198`). It is never inside the notes folder
(ADR-028), and the startup guard that refuses a data directory inside a sync provider's tree
(ADR-028 section 9) already covers it. `crates/writ-storage/src/chat_store.rs` owns the directory:
list, load, save, rename, delete, each save through the crate's atomic writer
(`crates/writ-storage/src/atomic.rs`). The schema is versioned:

```json
{
  "version": 1,
  "id": "…", "title": "…",
  "created_at": "…", "updated_at": "…",
  "provider": "anthropic", "model": "claude-sonnet-5",
  "turns": [
    { "role": "user", "content": "…",
      "attachments": [{ "path": "…", "bytes": 1234, "hash": "…" }] },
    { "role": "assistant", "content": "…",
      "proposals": [{ "path": "…", "summary": "…", "before_hash": "…",
                      "new_content": "…", "status": "applied" }] }
  ]
}
```

What it holds: the user's turns, the model's replies with the proposal fence already removed
(section 9), proposals with their status, and attachment references by path, size and hash. What
it never holds: an API key, and the text of an attached note. The note text is on disk in the
note, and a conversation that carried it would be a second copy ADR-028 forbids and a leak of
note text into a store ADR-031 rule 5.2 does not list. Rule 5.2 is amended to name this store as
the one place prompt and reply text persist (section 10).

The title is the first user turn's first line, cut at 60 characters, until the user renames it.
The list is the directory sorted by `updated_at`, read once and kept in memory while the pane is
open. Retention: a conversation stays until the user deletes it. No automatic pruning, no cap on
count, and one conversation is capped at 4 MB of JSON, after which a send is refused with a line
that says to start a new chat. Delete unlinks the file after a confirmation; there is no trash for
conversations because they are not notes. Every write to a conversation is autosave: after each
user turn, after each `done`, `stopped` or `error`, and after rename.

The store is exposed through five commands, `chat_list`, `chat_open`, `chat_new`, `chat_rename`
and `chat_delete`, beside the six that exist (`src-tauri/src/lib.rs:725-730`). The pane's store
loads the most recent conversation on open and creates one on the first send when none exists.

### 9. The reply is rendered, the fence is stripped where the stream is parsed, and a proposal shows a diff

**Rendering.** A reply is Markdown rendered by `writ-render`, so the chat and the preview agree on
what a fence, a table or a task list looks like. Model output is untrusted input (ADR-031 rule
4.1), so the chat calls a variant that drops `Event::Html` and `Event::InlineHtml`
(`crates/writ-render/src/lib.rs:405-407` passes them today, which is right for a note and wrong
for a reply). The fragment is inserted into the pane's own DOM, not the preview iframe, which
ADR-031 rule 1.6 keeps untouched. A link in a reply opens through the same external-link path the
editor uses (`src-tauri/src/commands/link.rs:71`) after a click, never on load. Each code block
gets a copy button; the copied text is the block's source, not its rendered HTML.

**Fence stripping.** `crates/writ-core/src/chat.rs` gains `ProposalFilter`, a state machine that
takes stream deltas and returns visible text. It buffers from a line-leading backtick run until
it can read the info string; a `writ-proposal` block is withheld to its closing fence, any other
block is released as it arrives. `src-tauri/src/commands/chat.rs:814-816` runs each delta through
it before `emit_chat`, and `parse_proposals` still reads the full accumulated reply on `done`.
Tests feed one recorded proposal reply split at every byte boundary and assert that no fence
character and no proposed line reaches the visible text, and that a reply with an ordinary code
block loses nothing. The persisted `content` (section 8) is the filtered text.

**Diff.** No diff implementation exists in the tree; the rewrite overlay replaces text inline and
compares nothing. `crates/writ-core/src/diff.rs` adds a line diff, Myers `O(ND)` with a guard at
ADR-031 rule 4.8's 2 MB, returning hunks of context, removed and added lines. The proposal card
carries the hunks, computed in Rust when the proposal is parsed, because both texts are already
there. The card shows the diff and the summary; it no longer shows two full texts. The rewrite
overlay may adopt the same hunks later, and this is the implementation it would adopt.

**The pane's other affordances**, each a rule for U2:

- A visible thinking state between Send and the first delta, on the assistant turn itself.
- Stop, as today. Retry on an error re-sends the last user turn with its attachments and does not
  clear the composer on failure. Editing a user turn truncates the conversation at that turn and
  re-sends; the truncated turns are gone from the file on the next autosave.
- `@` in the composer opens the note list the `[[` completion already builds
  (`src/editor/wikilink-complete.ts`) and attaches the chosen note by path. Attaching reads
  through `ReadNote` as today; the list comes from the frontend's own index read model, so the
  context builder's permission set does not change. An attached note appears as a chip beside
  the composer whether it arrived by `@` or by the list, and the send dialog counts it.
- The model picker in the composer is bound to `ai.chat.model` and to the same list the Settings
  dropdown reads. Picking the connection's own model clears the override.
- The pane has a default shortcut, `Cmd+Shift+A`, registered as `chat.toggle` so the shortcut
  editor lists it, and a View menu item beside Toggle Connections (`src-tauri/src/menu.rs`). With
  chat off, both open Settings at the chat group, as the existing "Chat is turned off" dialog
  does.

### 10. Amendments to ADR-031

- **Rule 2.1**, add: the model-list request, the reachability check and the OpenRouter code
  exchange are requests to the AI host the user configured, and are not new destinations.
- **Rule 2.4**, replace the last sentence with: "Writ listens on no port at rest. The one
  listener it ever opens is the OAuth loopback receiver of ADR-040 section 6, bound to
  `127.0.0.1` on an ephemeral port for the duration of one Connect flow the user started; it
  accepts one request and closes. The threat-model check is that no port is open at rest and none
  after a completed or abandoned Connect."
- **Rule 2.7**, new: "A request to a loopback address that carries no credential and no note text
  is not a destination under rule 2.1, because nothing leaves the machine. The two local runtime
  probes of ADR-040 section 4 are the only such requests: they carry no credential, no note text
  and no header beyond what the HTTP client adds to every request (host, accept, an empty user
  agent), and a probe that carried a key would be a send under rule 2.2."
- **Rule 5.2**, add: "The conversation store of ADR-040 section 8, under `<writ_dir>/chats/`, is
  the one place prompt text and reply text persist. It holds no API key and no attached note
  text."
- **Rule 7.1**: `ai.enabled` reads `ai.rewrite.enabled`; `ai.chat.enabled` is unchanged. Both
  default to `false`.

### 11. What a connection change and a refusal do, amended 2026-09-17

An operator run against a DeepSeek key answered `400` on every send. Two defects produced it,
each on its own: a chat model picked under one provider stayed in the file after the provider
changed, and the pane's model list was fetched once per pane open, keyed by nothing, so ids
read from Ollama stayed on offer under DeepSeek. The status line the pane showed said only
"The model server returned status 400", and the record's rule 5.2 is why the sentence that
would have explained it was discarded. The following amend sections 1, 3 and 9 of this record.

- **The chat model override carries the provider it was picked under.** `[ai.chat]` gains
  `model_provider`, and `AiChatConfig::override_for(provider)` answers the override only while it
  matches the connection. A file written before the field existed has its override qualified to
  the provider that file names, once, in `AiConfig::from(AiConfigOnDisk)`, which stays the one
  migration point. A chat that names no model of its own carries no qualifier.

- **`AiConfig::with_provider` is the one place a provider changes.** It seeds the new row's model,
  keeps a hand-typed row's model because that row names no default, drops an override that does
  not belong to the new provider, and leaves consent and both feature switches alone. The
  settings panel and the composer's own control both reach it through `ai_set_provider`, so the
  clearing rule cannot be applied differently in two surfaces.

- **A model list is stamped with the provider it was read for.** `ai_list_models` answers a
  `ModelCatalog { provider, models, source, error }`, where `source` is `Live`, `Curated` or
  `None`. The frontend drops an answer whose `provider` is no longer the configured one, which
  makes an out-of-order response harmless by construction rather than by ordering. The curated
  ids move into the provider table of section 2, so the ids, the defaults and the wire come from
  one definition. One store holds the catalog for the settings panel and the pane.

- **A send is refused against a live catalog only.** `ModelUnavailable` is raised before the
  request is built and before a key is read, so a send that cannot work raises no keychain
  prompt. A `Curated` catalog is the table's suggestions and refuses nothing, because it is not
  the account's inventory. `ai_check_connection` reads the chat model as well as the connection's,
  and names whichever of the two the provider does not list.

- **A refusal names the reason, in Writ's words.** A non-2xx answer becomes
  `ProviderRejected { provider, status, code }`, where `code` is a `RejectCode` matched against a
  fixed allowlist parsed from `error.code` and `error.type` in the OpenAI-compatible envelope and
  `error.type` in Anthropic's. Anything outside the allowlist leaves `code` empty and the status
  stands alone. This narrows rule 5.2 of ADR-031: at most 8 KiB of a refusal body is read, and the
  only value that may leave the parser is one of six variants of a closed enum, so no response
  text can be shown, stored or logged whatever a host writes. `LocalServerOffline` and
  `EmptyModelList` join it, and each sentence is built from the provider table's label.

- **Every frame names the connection that produced it.** A request freezes
  `identity { provider, model, host }` when it is built; the accepted reply, the `done` frame and
  the assistant turn on disk all carry it, and an `error` frame carries
  `{ kind, message, provider, model, status }`. A reply says which model wrote it, and a refusal
  names the model that was refused. The turn field is optional, so conversations written before
  it open unchanged.

- **A request is identified by (conversation, request id).** The frontend mints the id, as it
  already does for `ai_rewrite`, and every `writ://ai-chat` frame carries it. A frame whose pair
  does not match what the pane is showing is dropped, so switching conversations mid-reply never
  crosses two streams, and Stop cancels one request rather than whatever is live under an id.

- **Applying a proposal reports what it did.** The parser widens to the fences models actually
  write and resolves the path it was given against the notes the request attached; proposals it
  could not place are named on the `done` frame with the reason, instead of disappearing. A write
  whose bytes match what the note already holds reports `changed: false` and the card says so,
  and the apply itself records the new disk state and tells an open note it changed, so the
  editor does not read the write back as an external edit.

## Consequences

**Callers.** `AiConfig` changes shape; every reader of `ai.preset`, `ai.enabled`, `ai.chat.provider`
and `ai.chat.base_url` in `src-tauri` and `src/` moves to `ai.provider`, `ai.rewrite.enabled` and
the provider table. `Provider::key_account` goes; the account is the provider id. The rewrite
stream learns the Anthropic wire. `src/types/config.ts` and `src/settings/index.ts` change ids and
keywords, and the two pinned tests are edited to the new vocabulary.

**Packaging.** No new crate. `writ-core` gains `ai/providers.rs`, `diff.rs` and `ProposalFilter`;
`writ-storage` gains `chat_store.rs`; `writ-render` gains a no-raw-HTML render variant;
`src-tauri` gains the model-list, probe, PKCE and conversation commands. `tauri-plugin-opener`
and `reqwest` are already dependencies; the loopback receiver uses `std::net::TcpListener` on a
thread, with no HTTP server crate.

**Tests.** One config-migration test per row of section 1's table. One keychain-migration test
per legacy account name against the in-memory store. Parser tests per wire family for the model
list, against recorded bodies. A probe test against a local listener that answers and one against
a closed port. PKCE tests for challenge derivation, state mismatch, timeout and single-accept.
Chat-store tests for list order, atomic save, rename, delete and the 4 MB refusal. The
byte-boundary fence test. Diff tests including empty sides, identical inputs and the 2 MB guard.
Architecture tests that `writ-core` still imports no `tauri` and no `reqwest`. The operator table
in `.status/reports/s5-group-end.md` gains six rows: chat against Ollama, chat against a hosted
key, a conversation surviving relaunch, Stop mid-stream, `@`-attaching an unopened note, applying a
proposal through the diff.

**Security.** One account per provider means clearing a key clears it for both features, which
is what a person expects. The consent sentence widens to name both features on one host. The
loopback receiver is the first socket Writ opens; it is bound to loopback, single-use, and
time-boxed, and the threat-model row is rewritten to check exactly that. Rendered replies are
untrusted; raw HTML is dropped before insertion and links open only on click. The conversation
store persists prompt and reply text on disk for the first time; it is inside the data directory
the sync guard already protects, and it holds no key and no note text.

**Negative.** Both-on-different-providers users (section 1, last row) pick a chat model once
more. The Gemini row carries two URLs, which is one more thing to keep right. A conversation file
is readable by any local program running as the user, the same as the notes and the activity log.
The 4 MB cap is a number picked before real conversation sizes are known.

## Open questions deferred to later records

- **GitHub Copilot through the Copilot SDK.** Permitted, documented, billed to the user's own
  subscription, published as a Rust crate, and a second sidecar plus an agent loop turned into a
  chat surface. A record of its own, prototyped against a throwaway GitHub App first.
- **Unmodified Claude Code as a subprocess** under Anthropic's Commercial Terms, if a Claude-plan
  story is ever wanted. Not this record.
- **Jan** as a third local runtime on port 1337. The probe pattern extends; the row is a one-line
  addition once someone asks.
- **Conversation retention controls** (a cap, an age, a "clear all") once real sizes are known.
- **The rewrite overlay adopting the line diff** from section 9.
