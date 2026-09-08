# ADR-032: The internal surface notes are reached through

## Status

Accepted, 2026-09-09.

Extends [ADR-006](./006-plugin-runtime-v1.md) and does not supersede it. ADR-006's rule is
repeated here rather than relaxed: no user-installable code, no dynamic loading, and no manifest
read from disk in the 0.5 release. The in-process transform runtime ADR-006 shipped, and the
composites [ADR-012](./012-composite-transforms.md) added to it, are unchanged by this record.

Sits under [ADR-031](./031-the-ai-harness-and-what-leaves-the-machine.md) rule 1.5, which states
the same boundary from the harness side, and inherits its write rules.
`docs/threat-model.md` is the checklist those rules are walked with.

[ADR-029](./029-notes-on-disk-direction.md) reserved this number for an external plugin API. The
0.5 release does not deliver one. It delivers the capability boundary such an API would have to be
built on, with two consumers inside Writ's own binary. Section 7 says why, and what an external
loader would still owe.

The trait is extracted in U9, after both consumers exist. Until then this record is the contract
they are built against.

## Context

`crates/writ-plugin` holds two shapes today. The transform runtime from ADR-006 is live:
`TextTransform` and `TransformRegistry` in `transform/registry.rs`, the built-ins in
`transform/builtins/`, and the `list_transforms` and `apply_transform` commands in
`src-tauri/src/commands/transforms.rs`. Beside it sits the stub ADR-006 found already in the crate
and left alone: `PluginApi` (`crates/writ-plugin/src/api.rs:9`) with three buffer-shaped methods,
and `PluginManifest` (`crates/writ-plugin/src/manifest.rs`). Nothing implements `PluginApi`,
nothing reads a manifest, and only a test in the crate names either.

The 0.5 release adds consumers that touch notes rather than a string. An MCP server answers eight
read tools (U4) and three write tools (U6). A chat pane assembles its request from attached notes
and applies proposals the user accepts (U7). Both list notes, read a note, search, read what the
index holds about a note, and write.

Left alone, each of them reaches `NotesIndexStore` and `writ_storage::guarded` directly, and two
costs follow. ADR-031 rule 4.3, that the chat pane never writes, stays a convention a reviewer
enforces. And what a consumer can do to a note becomes whatever its author imported, which is not
a thing a test can assert.

The decision is what those two consumers reach notes through, recorded before they are built so
that U9 is a move rather than a design.

## Decision

### 1. The surface is derived from two consumers and no more

The two are the MCP tool set (U4 for reads, U6 for writes) and the chat pane's context builder and
proposal applier (U7). A method neither of them calls does not exist on the trait, and a
capability that no operation maps to does not exist in the enum. Section 3 is the whole
derivation, and it is what U9 is reviewed against.

This is ADR-006's own move applied a second time: ship the narrowest surface that answers the real
feature, and do not anticipate capabilities before a caller exists. `PluginApi`'s three methods
are what anticipating looks like. They were written against no caller and have had no
implementation since.

### 2. Capabilities are the sandbox

```rust
pub enum Capability {
    ListNotes,
    ReadNote,
    SearchNotes,
    ReadIndex,
    WriteNote,
    CreateNote,
    RenameNote,
}

pub struct PermissionSet { /* a set of Capability */ }
```

A consumer holds a `PermissionSet`. The host checks it in the implementation, before the work,
rather than asking the consumer to check itself. There is no ambient authority: a consumer whose
set has no `WriteNote` has no code path to a write, because the only write path is behind the
check. A call without the matching capability returns `HostError::NotPermitted` and performs no
I/O, neither opening the file nor reading the index.

The set is not a second consent record. `writ_mcp::consent::ConsentGate` decides whether a client
may call at all and in which direction (ADR-031 rules 3.2 and 3.3); the permission set is that
decision in enforced form, derived from the client's approval when the host is constructed.

`ReadNote` and `ReadIndex` are separate grants because they have different backing and different
failure. With `writ.db` absent or unreadable, `list_notes` and `read_note` still answer by walking
the notes folder, while every index-derived tool returns `IndexUnavailable` (U4). `WriteNote`,
`CreateNote` and `RenameNote` are separate because a consumer approved to update a note it was
pointed at is not by that fact approved to add files to the folder or to move one.

### 3. The operation map

Every operation the two consumers name, the method it reaches, and the capability that method
checks.

| Consumer operation | `NoteHost` method | Capability |
|---|---|---|
| `list_notes(prefix, limit)` (U4) | `list_notes` | `ListNotes` |
| `read_note(path)` (U4) | `read_note` | `ReadNote` |
| `search_notes(query, limit)` (U4) | `search_notes` | `SearchNotes` |
| `note_links(path)` (U4) | `note_links` | `ReadIndex` |
| `note_backlinks(path)` (U4) | `note_backlinks` | `ReadIndex` |
| `note_properties(path)` (U4) | `note_facts` | `ReadIndex` |
| `note_tags(path)` (U4) | `note_facts` | `ReadIndex` |
| `folder_tags()` (U4) | `folder_tags` | `ReadIndex` |
| `write_note(path, content)` (U6) | `write_note` | `WriteNote` |
| `create_note(name, content)` (U6) | `create_note` | `CreateNote` |
| `rename_note(path, new_name)` (U6) | `rename_note` | `RenameNote` |
| attaching a note in the chat pane (U7) | `list_notes` | `ListNotes` |
| building the request from attached notes (U7) | `read_note` | `ReadNote` |
| applying a proposal (U7) | `write_note` | `WriteNote` |

Ten methods and seven capabilities. `note_properties` and `note_tags` are one method because
both read `NotesIndexStore::facts`, and the tool split belongs to the tool layer. Every capability
in section 2 is named by a row, and every row names a capability. U9 adds nothing to either
column.

### 4. Every write carries its origin through the one facade

The three write methods do not write. They call U3's facade,
`writ_storage::guarded::write_note_guarded` and `create_note_guarded`, with a
`writ_core::notes::WriteOrigin` naming the consumer: `WriteOrigin::Mcp { client }` for a tool
call, `WriteOrigin::Chat` for an applied proposal. The activity log and the history store are
populated from that one call by the host, so a consumer cannot write without being recorded, and
no consumer carries its own logging.

The conflict rule travels with it. Every write passes the caller's last known disk state as
`last_known`: the tool reads the current state itself (U6), and the chat pane passes the
proposal's `before_hash` (U7). A file changed since then is refused with a conflict copy rather
than overwritten. The trait has no force parameter, so there is no argument a consumer can pass to
make a refusal into an overwrite.

### 5. The chat surface holds two permission sets

The context builder holds `{ListNotes, ReadNote}`. That is the side a model's reply can influence,
and it has no write capability, which is what turns ADR-031 rule 4.3 into a property of the type.

Applying a proposal is a user action, and the applier holds `{WriteNote}` and nothing else. It
runs from the user's `Apply` and from nothing the model produced, passes the proposal's
`before_hash` as the last known state, and stamps `WriteOrigin::Chat`.

They are two sets because they answer to two actors. One set carrying `WriteNote` would give the
model-facing side a write path. One set without it would leave `Apply` writing outside this
surface, which is what section 4 exists to prevent.

### 6. Deletion, trashing and moving are not capabilities

The enum has no `DeleteNote`, no `TrashNote` and no `MoveNote`, and the trait has no method for
any of them, in the 0.5 release. This is ADR-031 rule 4.6 seen from the surface: the registered
write tools are `write_note`, `create_note` and `rename_note`. `rename_note` is the nearest thing
to a move that exists, it stays inside the notes folder, it stamps both paths in the ignore set,
and it does not propagate links (ADR-031 rule 4.7).

Adding any of the three is a new record, not a patch to this one. `writ trash`
(`crates/writ-cli/src/verbs.rs:70`) is unaffected: that is the user at their own terminal, not a
consumer of this surface.

### 7. The surface is internal, and nothing is published

The direction spec puts a plugin system out of scope
(`.status/specs/obsidian-direction-2026-08-22.md:17`) while the 0.5 checklist asks for a plugin
API defined from two internal consumers. The resolution is that this record delivers the second
without delivering the first.

No third-party code is loaded, nothing is installable, and no manifest is read from disk. The
surface is described here and nowhere a reader outside the repository sees it, and no
compatibility guarantee is attached to it, because it has no caller outside this tree. What ships
is a boundary inside Writ's own binary, which is a safety property rather than a platform.

ADR-029 reserved this record for an external, sandboxed, permission-scoped plugin API. The
permission scoping arrives here. The external loading does not, and ADR-006's precondition stands:
sandboxing is a precondition of the record that adds it, not an open problem this one has to
pre-solve. That record would also owe what a manifest declares, how a grant is asked for and
revoked, and what a loaded module can be denied after it is running. None of those questions has a
caller yet.

ADR-029 also asks what happens to the in-process transform registry. Nothing happens to it. It
stays what ADR-006 and ADR-012 describe, in `crates/writ-plugin/src/transform/`, with the host
surface re-exported beside it as a separate module (section 8). The two do not overlap, because a
transform sees a string and a host call sees a note.

### 8. Where the code lives

`pub trait NoteHost`, `Capability`, `PermissionSet` and `HostError` are declared in
`crates/writ-core/src/notes/host/`, beside `writ_core::notes::guard` and the `WriteOrigin` U3
puts in `crates/writ-core/src/notes/write_origin.rs`, which section 4's write methods carry.
`writ-plugin` re-exports them as `writ_plugin::host`, so a consumer reaching for the extension
surface still finds it in one crate. The one implementation, `NoteHostImpl`, lives in
`writ-storage`, which already owns `NotesIndexStore` and `writ_storage::guarded`. `src-tauri`
already depends on `writ-core` and `writ-storage` (`src-tauri/Cargo.toml:20`, `:23`) and
`writ-mcp` takes both when it is created (U4), so each constructs the implementation and holds it
as a `NoteHost`.

CLAUDE.md's boundaries hold unamended, and this split is what holds them. `writ-storage` keeps
`writ-core` as its only workspace dependency, because the trait it implements is a `writ-core`
type. `writ-plugin` keeps `writ-core` as its only workspace dependency, alongside the `serde`,
`serde_json` and `thiserror` it already carries. Neither crate points at the other. Declaring the
trait in `writ-plugin` instead would force a `writ-storage` edge to it, which CLAUDE.md's
`writ-storage` line forbids, and that is the reason the types sit where they do.

That constrains the signatures, and it is the part U9 has to get right. A trait in `writ-core`
cannot name a `writ-storage` type, and the index read model is `writ-storage`'s: `LinkRow`
(`crates/writ-storage/src/notes_index.rs:126`), `BacklinkRow` (`:180`), `NoteFactsRow` (`:211`),
and `StorageResult` itself. So the trait's return types are declared beside the trait,
`NoteHostImpl` maps the rows into them, and every method returns `Result<_, HostError>`.

`search_notes` returns a note-shaped hit of its own rather than `writ_core::search::SearchHit`.
That type is the editor's, and it leads with `buffer_id` and `title`
(`crates/writ-core/src/search.rs:35-37`), which names a copy of the text rather than the file
ADR-028 makes the only one. A consumer of this surface holds no buffer and opens none, so its hit
is a path, a line and an excerpt read from the file. Handing out a buffer id would be the mistake
section 1 records against `PluginApi`, made a second time.

### 9. When it lands

U9, once U4, U6 and U7 are in the tree and every method in section 3 has a working caller. The
trait is lifted from call sites that already run rather than written ahead of them and fitted
afterwards, which is the same rule ADR-006 applied to its own recording. U9 amends this record
with a shipped note in ADR-006's style, naming the trait in `writ-core`, the re-export in
`writ-plugin`, `NoteHostImpl` in `writ-storage`, and the two consumers. No `Cargo.toml` in the
workspace gains a path dependency in that unit, and CLAUDE.md is not edited.

U9 also removes what the extraction replaces. `PluginApi` (`crates/writ-plugin/src/api.rs:9`) has
no implementation and its methods are buffer-shaped rather than note-shaped, so it goes, and
`PluginManifest` with it unless something reads it by then. `writ-plugin` ends U9 with one host
surface, re-exported, rather than two half-surfaces of its own.

## Consequences

**Positive**

- "The chat pane cannot write" becomes checkable. The context builder's permission set carries no
  write capability, so a test asserts the rule that a reviewer used to.
- A capability no consumer uses cannot survive review, because section 3's table is the acceptance
  criterion and an unused row is visible in it.
- Both consumers reach notes the same way, so the containment rule, the conflict rule and the
  activity record are one edit in the implementation rather than one per consumer.
- U9 changes no user-visible behaviour, because the contract it lands was written before the code
  it lifts.

**Negative and risks**

- Seven capabilities is a coarse grant. `ReadNote` covers every note in the folder, with no
  per-folder or per-note scope, and adding one is a later record.
- A third consumer will find methods missing. That is the intended cost of section 1, and it is
  paid by amending this record rather than by widening the trait ahead of the caller.
- The trait's return types restate part of the index read model, so a change to `NoteFactsRow`
  means a change in two places until the shapes are shared.
- One trait with one implementation is indirection that buys nothing on the day it lands except
  the capability check, and the check is the whole reason it exists. A reader who expects a second
  implementation will not find one.
- The permission set is only as good as the consent it was derived from. ADR-031 rule 3.6 stands:
  a client that names itself after an approved one holds that client's capabilities until the user
  reads the activity log.
