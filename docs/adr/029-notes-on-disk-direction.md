# ADR-029: Notes on disk, with links, a graph, and an index a machine can read

## Status

Accepted, 2026-08-28.

Builds on [ADR-028](./028-files-are-the-only-copy.md), which makes the file on
disk the only copy of a note's text, and on
[ADR-027](./027-context-menus-and-rewrite-consent.md), whose consent pattern
governs any write Writ did not originate. ADR-030 records the design system this
direction is drawn in. Two records are reserved: ADR-031 for the MCP and AI harness
with its threat model, written before that code; and ADR-032 for the plugin
API, written once the two internal consumers that define it exist.

This ADR reverses four exclusions from the 2026-08-22 direction spec
(`.status/specs/obsidian-direction-2026-08-22.md`, gitignored). Nothing else in
that spec changes.

## Context

Two records describe the product and they disagree.

The **2026-08-22 spec** set the foundation that is still correct: a notes folder
of ordinary `.md` files with human names, files as the only truth, the word
`buffer` retired from user-visible strings, releases 0.4 through 0.8. Its
section 1 also drew a boundary, listing as out of scope a sync service of any
kind, a mobile app, a plugin system, a graph view, canvas, tasks, databases,
kanban, tags-as-a-system, any "second brain" framing, and any feature that
requires a folder-structure decision before the first sentence. Open question 6
closed with "ship L1 and L2 in 0.8, never ship a graph". The reasoning behind
that boundary was that the simplicity-seeker does not report missing links or a
graph, and that the two voices in the spec's research haul who mention a graph
call it a mess at scale.

The **maintainer's direction of 2026-08-25** (`.status/direction-2026-08-25.md`)
describes a larger product: Obsidian-class features for technical and
non-technical people in one major release, Writ as an AI-readable brain with a
viewer and a harness, better visualisation than Obsidian, privacy first, three
maintained desktop platforms, a web and a mobile version after that, and an
end-to-end encrypted cloud with passkeys as the monetisation path later.

The foundation survives that change. The exclusion list does not. Neither record
is an ADR, so the direction has been carried in gitignored files while the tree
and the public copy still describe the earlier product: `README.md:50` and
`README.md:56`, `site/src/components/site/Hero.astro:60`,
`site/src/pages/index.astro:167`, `site/src/pages/privacy.astro:21-23`, and
`src-tauri/tauri.conf.json:65-67`. Decision 9 states which of those change and
when.

## Decision

### 1. What Writ is

Notes on disk. Plain `.md` files in one folder the user can open in Finder, with
links between the notes, a graph over those links, an index a machine can read,
an MCP and AI harness over that index, and a plugin layer defined from it.

Privacy first is a property of the design, not a claim: no account, no
telemetry, no Writ server, and nothing leaves the machine unless the client the
user chose sends it. That sentence constrains decisions 4 and 6, and it is what
ADR-031 has to prove rather than assert.

The audience is technical and non-technical people. Developers stay supported:
`crates/writ-cli`, the watched folder of
[ADR-024](./024-inbox-arrival-snapshot.md), and default-app registration all
remain, and the CLI gains the index commands in decision 4b. They stop being the
audience the product is described to, which is a copy decision rather than a
feature decision.

### 2. Four exclusions from 2026-08-22 are reversed

The reason for all four is the maintainer's direction of 2026-08-25: Obsidian-class
features for technical and non-technical users, an AI-readable brain with an MCP
and AI harness, privacy first.

**Graph view.** The spec said never, in section 1, in L2 ("No graph, no local
graph, no map of content") and in open question 6. Writ ships a graph: the local
graph of the open note first, then a folder graph with search and folder
colouring. It is rendered in-house on a canvas from ADR-030 tokens, with no
third-party graph library, so the view is a query over decision 4b's index and
not a dependency carrying its own theme and data model. The spec's evidence
against graphs argues for shipping the local graph before the folder graph,
which is the order taken here, not against the feature.

**Plugin system.** [ADR-006](./006-plugin-runtime-v1.md) shipped an in-process
transform runtime in `crates/writ-plugin` and nothing external. Writ now gets an
external plugin API, recorded in ADR-032, sandboxed and permission-scoped, its
shape derived from the two internal consumers built first (decision 4d) rather
than designed ahead of any caller.

**Sync.** Reversed as a direction and deferred as work: an end-to-end encrypted
cloud with passkeys is the monetisation path in decision 6. Until it exists Writ
ships no sync service of its own.

**Mobile.** Reversed as a post-1.0 item in decision 6, connecting to storage the
user already owns rather than to anything Writ hosts.

One exclusion is reversed in part. The spec ruled out tags-as-a-system;
decision 4b's index carries a tags table, the sidebar lists tags, and the CLI
exposes `writ tags`. What stays out is a tag manager: no hierarchy editor, no
renaming scheme, no tag-driven organisation the user has to maintain. Tags are
read out of the files and shown.

The exclusions that stay are in decision 7.

### 3. Two releases: 0.4, then 1.0

**0.4 is ADR-028's scope alone.** Files become the only truth, the mirror is
retired, notes are created and renamed from inside the app, frontmatter survives
a round trip, search re-points at the folder, the write guard refuses to
overwrite a newer copy on disk, and the banned-word test lands. It ships on its
own and early because it is a data migration, and a migration wants the smallest
blast radius available. The index schema of decision 4b is created inside that
migration (ADR-028 section 11) so a second migration is not needed immediately
after.

**1.0 is everything else in decision 4.** Between the two, only fixes ship. A
release landing half the brain surfaces and none of the harness would describe a
product that does not exist yet, and the public claims in decision 9 all change
at once or not at all.

### 4. What 1.0 contains

Ordered by dependency, citing the 2026-08-22 spec by requirement ID. The
acceptance criteria there remain the tests.

**(a) Change handling.** W1 folder and open-file watching, the W2 conflict bar
(keep mine, use disk, show both), W4, W5, S1 and S2 save states and failure
reporting, C1 for dataless cloud files, R1, and M1 images. This comes before the
harness. An agent writing files under the user is exactly the case W1 and W2
exist for, and an MCP writer over a folder with no watcher and no conflict path
would put the one copy of the text behind an unguarded writer.

**(b) Links and the index.** L1 wikilinks and markdown links, resolved by name
and by path, in Obsidian-compatible syntax so an imported folder resolves without
a conversion step; L2 backlinks; L3 rename propagation, every rewrite going
through the guarded writer of (a). Behind them, index tables for links,
frontmatter properties, tags and headings, each keyed by canonical path and kept
current by the watcher. This is the data model everything after it reads: the
graph is a query over it, the right panel is a view of it, and the MCP tools are
its remote interface. The link resolver follows the injected-dependency seam
already used by `src/editor/link-layer.ts` so the frontend layering tests keep
holding. The CLI gains `writ links`, `writ backlinks`, `writ properties` and
`writ tags`, each with JSON output, which is developer parity and the groundwork
for (d) at the same time.

**(c) Brain surfaces.** A right panel with backlinks, outline, and properties as
a read-only list; tags in the sidebar; the local graph of the open note, then the
folder graph; and the preview upgrades that make an imported folder legible:
callouts, mermaid, embeds of other notes, tables, math, and M1's
containment-checked asset route in `src-tauri/src/preview/handler.rs`. The
acceptance statement for the set is that an Obsidian folder opens with its links,
properties and tags intact.

**(d) The AI harness.** An MCP server bundled with the CLI and the app, exposing
list, search, read, links, backlinks, properties and tags for reading, and
write, create and rename through the same conflict guard as the editor. Consent
is per client, and an activity log in the app shows what each client did. The AI
chat pane brings the user's own key or a local model, reads through the same
tools, and never writes directly: its writes are proposals the user accepts,
reusing ADR-027's consent pattern rather than inventing a second one. ADR-031
carries the threat model and is written before the code.

**(e) The plugin API.** Defined from (d)'s two internal consumers once both
exist, sandboxed and permission-scoped, so no code runs over the user's notes
without a grant. Recorded in ADR-032. An in-house engine rewrite is considered
only if the plugin surface proves that CodeMirror or `crates/writ-render` is the
limit; it is not a goal in itself.

**(f) First run and approachability.** O1 (a first launch that asks for
nothing), O2, V1's row renames, K1's shortcuts and menu bar, A1's accessibility
and interface text size, O3's notes-folder settings row, and D1 the daily note.

**(g) History.** H1, per-file, restore writing through the guarded path of (a),
with the policy in `crates/writ-core/src/history/`, a placeholder today.

**(h) The site rebuild.** Generated tokens replacing the hand-copied values in
`site/design-system/colors_and_type.css`, the route verdicts applied (`/`, `/vs/obsidian` and
`/privacy` rewritten, `/docs` split, `/vs/typora` and `/vs/apple-notes` cut),
and the copy rewritten for decision 9.

### 5. Platforms

macOS, Windows and Linux desktop, all three maintained. The design is OS-aware
from the token layer up: fonts, window chrome, control metrics and focus
treatment differ per platform by design, recorded in ADR-030.

This corrects the spec's open question 8, which recommended keeping Windows and
Linux while treating them as unequal in the first-run work. Unequal research is a
gap to close, not a tier to formalise. The platform-specific parts of O1 and O2
are researched for macOS only today, so that research is owed before (f) ships.

### 6. After 1.0

In this order, each depending on the one before:

1. **A web version** storing in session storage. No account, nothing persisted
   server-side. It reuses the token contract and the render crate; what it needs
   is a storage adapter and a sidebar.
2. **A mobile version** connecting to storage the user already owns: Google
   Drive, iCloud Drive, or a folder provider. Writ holds no copy.
3. **An end-to-end encrypted cloud with passkeys** and transparent disclosure of
   what is stored and what the server can read. This is the monetisation path,
   last because it is the only part of the product asking the user to trust
   something other than their own disk.

Before that cloud tier exists, Writ ships no sync service. The answer stays the
one O3 already wrote: Writ has no sync; put the notes folder in iCloud Drive,
Dropbox or Google Drive and the notes go with it.

### 7. What stays out

- **Templates.** D1 ships the daily note and nothing else. The evidence for
  templates is a mention inside the daily-note requests and nothing standalone,
  and a template engine is the first step toward the
  productivity-operating-system failure the audience names directly.
- **Canvas, tasks, databases, kanban.** Each is a second document type or a
  second data model, and none of them is a note in a folder. They would make the
  folder stop being the product.
- **Any feature that forces a folder structure before writing.** No `New folder`
  command, no vault setup, no picker on first run. Structure paralysis is the
  named failure O1 exists to avoid; Writ lists, searches, links and opens
  subfolders the user made in Finder and creates none.
- **A sync service of Writ's own before the cloud tier.** Sync without an
  end-to-end encryption story and a disclosure page is the one feature that
  would contradict decision 1.
- **Third-party graph and plugin runtimes.** A graph library brings its own
  theme and data model into a surface that has to read decision 4b's index and
  be drawn in ADR-030's tokens. A third-party plugin runtime would import a
  permission model that was not designed against Writ's notes folder.

### 8. Vocabulary

`vault` and `second brain` never appear in user-visible copy. `buffer`,
`scratchpad`, `inbox` and `render surface` are retired from user-visible strings
and stay only as internal type names. ADR-028 section 10's banned-word test is
what enforces this, and it lands in 0.4, before the largest new string surface
is written.

`brain` is allowed as a product word only where it is what the user calls their
own folder. The public name for that concept is decided inside decision 4c's
brain-surfaces work and applied to copy before 1.0.

The phrase `Obsidian alternative` is not used in Writ's own copy. The
Obsidian-compatible syntax in decision 4b is a compatibility fact, stated where a
user asks whether their folder will open. It is not a positioning line.

### 9. Public claims change with 1.0, not before

Nothing public changes until 1.0. Release 0.4 changes the storage model but not
the product description, and a positioning change announced ahead of the features
it describes is a claim Writ cannot back.

At 1.0 these change together:

- `README.md:50`, "Wikilinks and backlinks are on the way. They are not in a
  release yet." True today and correct to keep through 0.4. It becomes false the
  day L1 and L2 ship, and is rewritten then rather than retracted.
- `README.md:56`, the storage paragraph describing `~/.writ/buffers/` and the
  buffers directory as a scratch layer. False after ADR-028; rewritten to the
  notes folder and a derived index.
- `site/src/pages/privacy.astro:21-23`, that `writ.db` holds a copy of the text
  that full-text search reads. After 0.4 the database holds an index, history and
  window state, and the page has to say so. The same page's line 30, on piped CLI
  input being written to `piped/` and kept there, changes with it.
- `site/src/components/site/Hero.astro:60` and
  `site/src/pages/index.astro:167`, both selling a scratchpad over whatever the
  user is doing, the second while using the word `vault`.
- `src-tauri/tauri.conf.json:65-67`: `category: "DeveloperTool"`,
  `shortDescription: "Lightweight text editor for developers"`, and a
  `longDescription` built on "scratchpad editor for developers". These reach
  Finder, Spotlight and Windows metadata, so they are a public claim even though
  no page renders them.

At 1.0 these become sayable, each only once it is true: notes are ordinary `.md`
files in a folder the user chooses; a note changed outside Writ is noticed, and
unsaved edits are never replaced without asking; versions of a note are kept for
up to 30 days within the per-note and store caps; notes link to each other by
name and the links are visible as a graph; an MCP client the user chooses can
read and write the folder, with consent per client and a log of what it did.

These stay unchanged: the install card's platform facts, the offline claim, the
open-source claim, the notarised badge, and the four-format live render claim.

## Consequences

- The two things this direction is named for, a graph and an AI harness, both sit
  on a data model that does not exist in the tree today. Decision 4b comes before
  every surface that reads it, and nothing in 4c starts before it lands.
- The harness cannot ship before change handling. Agents writing files under the
  user is the case W1 and W2 exist for, so 4a gates 4d however early the harness
  spec is written.
- What it buys: one index serves the graph, the panels, the CLI and the MCP
  tools, so each new surface is a query rather than a subsystem, and the guarded
  writer means an external agent gets no more power over the text than the editor
  has.
- 0.4 ships a data migration with no new user-visible feature. Its release notes
  are about storage, and the product they describe is the current one.
- The gap between 0.4 and 1.0 is long, and only fixes ship in it. That is the
  cost of changing every public claim in one release rather than in pieces.
- Merging this ADR makes the direction public in the repository before the site
  says it. Anyone reading `docs/adr/` learns the positioning ahead of the
  announcement. That is accepted: an ADR that waits for the copy is not a record.
- ADR-006's plugin runtime stops being the whole plugin story. ADR-032 has to say
  what happens to the in-process transform registry once an external API exists.
- The graph reverses a position the spec argued for on evidence. If the folder
  graph turns out to be the mess that evidence describes, the local graph is the
  part that survives, which is the reason for shipping it first.
- Three maintained desktop platforms plus a web and a mobile target multiply
  every interface decision. ADR-030's platform layer is what keeps that from
  becoming three codebases.
- The trigger for reopening this record is a maintainer reversal, which is how it
  was opened. Nothing in the evidence collected for the 2026-08-22 spec has been
  shown to be wrong; what changed is what the product is for.
