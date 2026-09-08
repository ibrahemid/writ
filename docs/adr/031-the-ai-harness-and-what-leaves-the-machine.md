# ADR-031: The AI harness, and what leaves the machine

## Status

Accepted, 2026-09-09.

Extends [ADR-027](./027-context-menus-and-rewrite-consent.md), which set the consent shape this
record generalises, and stands on [ADR-028](./028-files-are-the-only-copy.md) section 1, which
makes the file on disk the only copy of the text. ADR-028's follow-on line names this record.
[ADR-006](./006-plugin-runtime-v1.md) is not superseded; the internal host API is recorded
separately in ADR-032.

The rule numbers below are the contract the units that build the harness are reviewed against.
Each rule names either the code that already satisfies it or the unit that will. Rules 3.1, 3.6,
4.1 and 4.2 are the premises the rest are argued from rather than work an implementer does, so they
name neither. `docs/threat-model.md` is the checklist those rules are walked with.

## Context

Half the harness already ships. `src-tauri/src/commands/ai.rs` is a working rewrite subsystem:
eight IPC commands (`ai_set_api_key:216`, `ai_clear_api_key:248`, `ai_has_api_key:263`,
`ai_endpoint_state:327`, `ai_consent_host:355`, `ai_rewrite:694`, `ai_cancel:751`,
`ai_check_connection:936`), registered at `src-tauri/src/lib.rs:712-719`, with policy in
`crates/writ-core/src/polish.rs` (`resolve_endpoint:146`, `is_endpoint_allowed:223`). Keys live in
the OS keychain (`mod keychain`, `ai.rs:81`, service `com.writ.ai` at `:86`) with a session-memory
fallback (`AiState.keys`, `:50`) for platforms with no native store (`:115`). Consent is per host,
recorded host-side so the stored string is by construction the string the guard checks
(`crates/writ-core/src/config/ai.rs:56`, `ai_consent_host:355`), and asked at the moment of
sending (`src/commands/ai.ts:26`, dialog at `:45-51`). The file header at `ai.rs:9-22` already
states the privacy invariants for that one path.

What arrives in 0.5 is the rest: an MCP server other programs call, a chat pane the user drives, an
activity log, and per-file history. Four new writers and two new readers reach notes in one
release. Each of them is a place where note text can leave the machine, or where a program the
user did not audit can change a file.

The rules have to exist before the code, for two reasons. The units are reviewed against them
rather than against each other, and the privacy claim the site makes about the harness is the
claim this record has to be able to support literally.

## Decision

### 1. The harness is three surfaces and one boundary

1.1. The harness is exactly three surfaces: an MCP server other programs speak to (U4 for reads,
U6 for writes), a chat pane the user drives (U7), and the activity log that records both (U5). No
fourth surface reaches notes in 0.5.

1.2. The boundary is the note file. Every surface reads and writes through the same paths the
editor uses. No surface gets a private read path, a private write path, or a cache of note text of
its own. Writes go through the single guarded facade (U3), which is the only caller of
`writ_core::notes::guard::decide_save:65` once U3 lands; it exists today in three copies
(`crates/writ-storage/src/buffer_store.rs:638`, `crates/writ-storage/src/note_ops.rs:187`,
`:357`).

1.3. The MCP process reads the index read-only through `NotesIndexStore::open_read_only`
(`crates/writ-storage/src/notes_index.rs:1660`) and writes `writ.db` never. Two processes do not
write one SQLite file in this release (U4).

1.4. A running app learns about an outside write through the notes watcher
([ADR-033](./033-external-change-handling.md)), not through a channel the harness adds. No socket,
no shared lock, no new IPC between the server process and the app (U6).

1.5. The plugin surface this harness is built on is internal. No third-party code is loaded,
nothing is installable, no manifest is read from disk, and no surface is documented for anyone
outside the repository. What ships is a capability boundary inside Writ's own binary, which makes
"the chat pane cannot write" a property of the type rather than a convention (U9, recorded in
ADR-032).

1.6. The chat pane occupies its own resizable column, closed by default. The `writ-preview://`
iframe is hidden with `hidden` when the layout needs the room and is never removed, because
removing a loaded preview iframe freezes the macOS webview (U7).

### 2. Nothing leaves the machine unless the user's chosen client sends it

2.1. Writ's own outbound requests are enumerated, and there are two destinations. The AI host the
user configured, reached from the rewrite stream (`src-tauri/src/commands/ai.rs:547`, sent at
`:552`) and from the reachability probe (`:853`, sent at `:858`); and the update endpoint, reached
from `updater.check()` (`src-tauri/src/commands/update.rs:200`) and from the install download
(`:108`), both built by `build_updater:266`. Adding a third destination is an ADR, not a patch.

2.2. The probe counts as a send. It carries the API key (`ai.rs:13-15`), so it is gated by consent
exactly as a rewrite is, and the shipped dialog says so in the copy the user reads
(`src/commands/ai.ts:49`).

2.3. The MCP server adds no outbound request of any kind. It speaks the protocol over stdio to a
process the user launched. Where that process sends the bytes afterwards is that program's
business and that program's own consent prompt, not Writ's (U4).

2.4. The server is a stdio process the client starts, not something the app hosts and not a
listener. A GUI process has no client stdio to attach to, so shipping the server with the app means
shipping the `writ` binary and showing the exact command to paste into the client's configuration
(`crates/writ-cli/src/main.rs:138`, U4 for the subcommand, U5 for the settings row). Writ opens no
port, and a check that it opens none is in the threat-model checklist.

2.5. The chat pane assembles its request from the notes the user attached and nothing else. No
folder sweep, no index dump, no silent inclusion of neighbouring notes. The pane shows which notes
are attached, and attaching one is a user action (U7).

2.6. An endpoint that is not local and not `https` is refused before any bytes leave, by
`polish::is_endpoint_allowed:223` against the parsed host, including for a hand-edited
`config.toml`. The chat pane reuses that guard rather than adding a second one (U7).

### 3. A client is untrusted input

3.1. MCP `clientInfo` is a name a program chose for itself. It is treated as a label, never as an
identity.

3.2. Consent is trust on first use, granted by the user inside Writ, per client. An unknown client
is `Pending`: its call is refused, and one pending record is appended so the user can see it and
decide (U4 declares the gate, U5 implements it against `[mcp] approved_clients`).

3.3. Read and write are separate permissions. Approving a client for reading never grants it
writing (U5).

3.4. Approval is granted only in the app. The CLI writes no approval, and no protocol message can
grant one (U5).

3.5. The live gate re-reads the approval list, so approving a client in the app takes effect on its
next call without restarting it. The read is guarded by the config file's mtime, so an unchanged
file costs one `stat` (U5).

3.6. Naming yourself after an approved client is the attack this design does not prevent. The
mitigation is that the user sees the pending row and the approved list, not that the name is
verified. This is stated rather than papered over: a local program that can read the user's config
can also read the notes folder directly, so client-name spoofing buys an attacker nothing it did
not already have.

3.7. Every path argument from a client is canonicalised and checked against the notes root before
anything opens it, reusing the containment rule at `src-tauri/src/commands/notes.rs:916`. A path
outside the root is refused, and a symlink is refused after canonicalisation, not before (U4).

### 4. Prompt injection is in the threat model

4.1. Note content reaching a model can carry instructions aimed at the model. Writ assumes it
does.

4.2. The mitigation is structural, not textual. No prompt wording is relied on to keep a model in
line, because a system prompt is advice and note content is input.

4.3. The chat pane never writes. It produces a `Proposal { path, before_hash, new_content,
summary }` that the user reads beside the current text and applies by hand. Applying goes through
the guarded facade with `before_hash` as the last known state, so a note changed since the proposal
was made is refused with a conflict copy rather than overwritten (U7).

4.4. There is no always-apply setting, and none is added later without its own record. An
always-apply switch is the one place a model writes to a file with nobody watching (U7).

4.5. MCP write tools are a separate consent from read tools, and are how a client the user
deliberately approved writes (rule 3.3, U6).

4.6. No tool deletes or trashes a file. The tool list is `write_note`, `create_note` and
`rename_note`, and a test asserts no other write tool is registered (U6). `writ trash`
(`crates/writ-cli/src/verbs.rs:70`) stays what it is today: the user at their own terminal, not a
client.

4.7. `rename_note` moves a file, so it goes through the same guard as any write and stamps both
the old and the new path in the ignore set. Link propagation is not offered to a client: rewriting
links across a folder is a user-facing offer with a count and an undo, and a silent bulk rewrite
triggered by a model is the failure this rule exists to prevent (U6).

4.8. No tool reads a file larger than 2 MB, and none returns a file outside the notes root
(U4).

### 5. What is logged, and what is not

5.1. The activity log records time, actor, action, note path, decision and byte count. The record
type carries no field capable of holding note text (`writ_core::activity::ActivityRecord`, U5), so
this rule is enforced by the type rather than by review.

5.2. No note content, prompt text, response text or API key reaches `config.toml`, `writ.db`, the
activity log, a `tracing` line, or an error string shown to the user. Lengths, hashes, paths, tool
names, client names and status codes are the loggable set. The rewrite path already holds this line
(`ai.rs:21-22`), and the updater already redacts URLs from its errors
(`sanitize_update_error`, `src-tauri/src/commands/update.rs:293`).

5.3. Errors shown to the user name hosts, paths and status codes only, the way
`sanitize_update_error` (`src-tauri/src/commands/update.rs:293`) already redacts an updater error
(U5, U6, U7).

5.4. The log is append-only JSONL in Writ's data directory, written with `O_APPEND` so the app and
a CLI-hosted server process can both append without a lock. It rotates at 5 MB and keeps one
generation. A malformed line is skipped, never fatal (U5).

5.5. Every tool call is recorded, whether it was allowed, refused or left pending, and so is every
chat proposal, whether it was applied or discarded (U5, U6, U7).

5.6. Per-file history stores content, so it is bounded and it stays out of the notes folder.
Content-addressed blobs and a `history.db` index live in Writ's data directory, so a sync client
never carries them and deleting the notes folder never deletes its own history. Retention is fixed
at 30 days, 200 versions per note, and 250 MB in total, pruned oldest first by age, then by note
over its cap, then globally by size. A note over 2 MB is not versioned. The numbers are stated in
one read-only settings line and are not configurable in 0.5 (U8).

### 6. Consent is revocable in the app and by editing the config

6.1. Every consent record lives in `config.toml`, in plain text the user can read and edit:
`ai.consented_hosts` for hosts (`crates/writ-core/src/config/ai.rs:56`) and `[mcp]
approved_clients` for clients (U4 declares the section, U5 fills it).

6.2. Every consent record is revocable from inside the app: a host through the AI settings section
that ADR-027 section 2 put there, a client through the connected-programs section, per direction,
with a control that forgets it entirely (U5).

6.3. Revoking takes effect on the next call, not on the next restart (rule 3.5).

6.4. Consent is recorded per host and per client, never once for the feature. Consenting to one
provider never covers another, which is the shipped behaviour the config comment already states
(`config/ai.rs:52-54`).

### 7. Everything is off by default

7.1. `ai.enabled` is `false` (`crates/writ-core/src/config/ai.rs:12`), `mcp.enabled` is `false`
(U4), and `ai.chat.enabled` is `false` (U7). A fresh configuration reaches no network and answers
no client.

7.2. With `mcp.enabled` false, every tool returns `NotApproved`. Turning it on does not approve a
client; rule 3.2 still applies to the first call from each one (U4, U5).

7.3. Per-file history is the one thing on by default, because a version store that the user has to
find and enable is a version store that is empty on the day it is needed. It writes only inside
Writ's data directory and never inside the notes folder (U8, rule 5.6).

7.4. Every new configuration field has a serde default, so an existing `config.toml` upgrades
without an edit (`crates/writ-core/src/config/mod.rs:575`).

## Consequences

**Positive**

- The privacy claim is one sentence that can be checked: two destinations, both configured by the
  user, and the note harness adds neither.
- The type carries the logging rule. An implementer who wants a better log line finds no field to
  put note text in.
- A client that has not been approved cannot read a note, and a client approved to read cannot
  write one, without a second deliberate act by the user.
- One guarded write path means a conflict is resolved the same way whether the editor, a client,
  the chat pane or a restore triggered it, and every resolution leaves the losing side on disk.
- History being on by default is what makes the rest of the harness safe to use: a write that a
  user regrets is recoverable in the app.

**Negative and risks**

- Client identity is a self-declared name. A local program that impersonates an approved client
  gets that client's permissions until the user notices the activity log.
- The pending flow costs a round trip. A client's first call fails, and its author has to expect
  that and retry after the user approves.
- Proposals are slower than applying an edit. A user who wants a model to edit many notes has to
  approve a client for writing and use the tools, which is the deliberate cost of rule 4.4.
- Fixed retention means a user who wants a year of versions cannot have one in 0.5. Making the
  numbers configurable is a later change once real store sizes are known.
- The activity log is not tamper-evident. Any local program running as the user can rewrite it,
  the same as it can rewrite the notes.
- A refusal from an MCP tool reaches the user through the client's own error rendering, which Writ
  does not control.

**Follow-on records**

- ADR-032 records the internal host API and the capability set, and states which of ADR-006 it
  extends.
- `docs/threat-model.md` is the checklist these rules are walked with at the end of the release.
