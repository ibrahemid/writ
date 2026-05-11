# ADR-006: Plugin Runtime v1 — In-Process Text Transforms

**Status:** Proposed
**Date:** 2026-05-08

## Context

`writ-plugin` exists as a stub: a `PluginManifest` struct and a read-biased
`PluginApi` trait. No plugin has ever been loaded. No host code consumes the
crate. The boundary is declarative; there is no runtime.

The next concrete user-visible feature that depends on this crate is a small
suite of palette-driven text operations: trim leading whitespace per line,
normalize whitespace, smart-quote → straight-quote conversion, and dedent.
Today these would have to live somewhere arbitrary — a `commands/` module in
`src-tauri`, an ad-hoc helper in `writ-core`, or hand-rolled JS in the
frontend. None of those locations are right. Each one couples a
self-contained string transformation to a layer that should not own it, and
each forecloses on the eventual delivery of these same operations through an
external plugin (WASM, JS, or dynlib) without a rewrite of every call site.

The decision in front of us is what the plugin crate should expose **first**.
The wrong move is to define a generic `Plugin` trait now and try to
anticipate every future capability (commands, decorations, language support,
LSP-shaped things) before any of them exist. That ADR cannot be written
honestly today; we have one capability to ship and one only.

The right move is to define the **narrowest stable surface that solves the
real feature**, and to do it with the future-loader question explicitly in
mind so that the registry shape we ship does not have to break when an
external loader arrives.

## Decision drivers

- **Sync-only**: text transforms run on a buffer slice and return a string.
  No I/O, no network, no async, no streaming. If a future transform needs
  any of those, it gets its own surface (and its own ADR).
- **Sandboxing deferred**: every transform in v1 is built into the host
  binary and ships with Writ. No user-installable code. No permissions
  dialog. No capability declarations in the manifest. If we add external
  loading later, sandboxing is a precondition for that ADR — not an open
  problem this one has to pre-solve.
- **Palette-first UX**: the canonical way to invoke a transform is the
  command palette. Each registered transform becomes a `Command` entry on
  the frontend. Selection-aware execution (apply to selection if present,
  else whole buffer) is owned by the frontend; the Rust trait sees only
  `&str` in, `Result<String, _>` out.
- **Loader-agnostic registry shape**: a future WASM/JS/dynlib loader must be
  able to register entries into the same registry without changing the
  `TransformRegistry` API or the `TextTransform` trait. The trait must be
  implementable by an adapter that wraps an external module — meaning no
  `'static` references to caller-owned data, no associated types, and no
  reliance on host-only capabilities at the trait boundary.
- **No `tauri` in `writ-plugin`**: ADR-005 is the law. The registry is
  declared and tested in `writ-plugin` with `writ-core` as its only
  workspace dependency. The host instantiates the singleton in `src-tauri`
  and exposes IPC commands; the registry itself never sees Tauri.

## Considered options

### Option A — Trait-object registry (`Box<dyn TextTransform>`)

```rust
pub trait TextTransform: Send + Sync {
    fn id(&self) -> &str;
    fn metadata(&self) -> &TransformMetadata;
    fn apply(&self, input: &str) -> Result<String, TransformError>;
}

pub struct TransformRegistry {
    transforms: HashMap<String, Box<dyn TextTransform>>,
}
```

Each built-in is a unit struct that implements the trait. Registration is
`registry.register(Box::new(TrimLeadingWhitespace::default()))`. Lookup is
`registry.get(id)`. The registry is `Send + Sync` and lives behind a `Mutex`
or `RwLock` in `AppState`.

A future WASM loader implements `TextTransform` once on a host-side adapter
(`WasmTransform`) that wraps the loaded module's exported function. The
registry never learns about WASM.

- Pros: trivially extensible, identical surface for built-ins and future
  loaders, idiomatic Rust, easy to test in isolation.
- Cons: heap allocation per transform (negligible — there are tens, not
  thousands), virtual dispatch on every call (also negligible — transforms
  run at user-action cadence).

### Option B — Tagged-union enum (`enum BuiltinTransform`)

```rust
pub enum BuiltinTransform {
    TrimLeadingWhitespace,
    NormalizeWhitespace,
    SmartToStraightQuotes,
    Dedent,
}

impl BuiltinTransform {
    pub fn apply(&self, input: &str) -> Result<String, TransformError> { ... }
}
```

Closed set of variants. No allocation. Match-based dispatch.

- Pros: no heap, exhaustive matching, smallest possible v1 footprint.
- Cons: closed to the world. A WASM loader cannot add a variant at runtime;
  the enum would have to grow a `BuiltinTransform::External(Box<dyn ...>)`
  arm or be wrapped in an outer `enum Transform { Builtin(...), External(...) }`.
  Either path is a breaking change to every match site the day external
  loading lands. **This is exactly the future break the epic told us to
  avoid.**

### Option C — `dyn Fn` pointers (`Box<dyn Fn(&str) -> Result<String, _>>`)

```rust
pub struct TransformRegistry {
    transforms: HashMap<String, Box<dyn Fn(&str) -> Result<String, TransformError> + Send + Sync>>,
    metadata: HashMap<String, TransformMetadata>,
}
```

Each built-in registers a function pointer and a separately-keyed metadata
record.

- Pros: minimal type machinery, easy to register inline.
- Cons: metadata and behavior are split into two parallel maps that can
  drift; no place to hang per-transform state (e.g., a precompiled regex)
  without external `Lazy` statics; a WASM adapter that needs to hold a
  module handle has nowhere clean to put it; harder to extend later
  without breaking the registration call sites.

## Decision

**Option A — trait-object registry.**

`writ-plugin` defines:

```rust
pub trait TextTransform: Send + Sync {
    fn id(&self) -> &str;
    fn metadata(&self) -> &TransformMetadata;
    fn apply(&self, input: &str) -> Result<String, TransformError>;
}

pub struct TransformMetadata {
    pub label: String,
    pub description: String,
    pub category: TransformCategory,
}

pub enum TransformCategory { Whitespace, Punctuation, Indentation, Other }

pub enum TransformError {
    InvalidInput { reason: String },
    Internal { reason: String },
}

pub struct TransformRegistry { /* HashMap<String, Box<dyn TextTransform>> */ }
impl TransformRegistry {
    pub fn new() -> Self;
    pub fn register(&mut self, transform: Box<dyn TextTransform>) -> Result<(), RegistryError>;
    pub fn get(&self, id: &str) -> Option<&dyn TextTransform>;
    pub fn list(&self) -> Vec<&TransformMetadata>;
}
```

Built-in transforms are unit structs in a `builtins` module of
`writ-plugin` (single crate; size does not justify a separate
`writ-plugin-builtins` crate at v1 — revisit if the module crosses ~400
LOC or grows non-trivial dependencies). Each implements `TextTransform`
and is registered by a `register_builtins(&mut TransformRegistry)`
helper.

The host (`src-tauri`) holds one `TransformRegistry` behind a `RwLock`
in `AppState`, populates it once at startup via `register_builtins`, and
exposes two IPC commands:

- `list_transforms() -> Vec<TransformDescriptor>` — id + metadata only.
- `apply_transform(id: String, input: String) -> Result<String, String>` —
  the input string is supplied by the frontend (the selection text, or
  the full buffer text if no selection). The frontend is responsible for
  knowing where to splice the result back. This keeps the IPC stateless
  with respect to buffer storage and avoids any read-modify-write coupling
  between transforms and the autosave path.

Note this signature differs from the epic prompt's suggested
`apply_transform(transform_id, buffer_id, range)`. Passing the input
text directly is simpler, removes a buffer-storage round trip, and
sidesteps staleness when the user has unsaved edits in the buffer. The
range stays where it belongs: in the frontend, alongside the selection
that produced it.

## Rationale

The trait-object registry is the only option of the three where a future
external loader plugs in **without changing the registry surface, the
built-ins, or the call sites**. That is the load-bearing constraint of
this ADR. A WASM loader is implemented as a single `WasmTransform`
adapter struct that holds a module handle and implements `TextTransform`
the same way `TrimLeadingWhitespace` does. The registry, the IPC
commands, and the frontend palette do not learn that anything new
happened.

Heap allocation and virtual dispatch are not real costs at user-action
cadence on text smaller than a buffer. Option B's enum saves both at the
price of closing the system, which is exactly what the epic forbids.
Option C's `Fn` pointers save type machinery at the price of splitting
metadata from behavior and giving adapters nowhere to hold state — both
of which we will pay for when the external loader arrives.

## Consequences

### `writ-plugin`

- New public surface: `TextTransform` trait, `TransformRegistry`,
  `TransformMetadata`, `TransformCategory`, `TransformError`,
  `RegistryError`, `TransformDescriptor`, and a `builtins` module with
  the four v1 transforms plus a `register_builtins` helper.
- The existing `PluginApi` trait and `PluginManifest` stay where they
  are. They are not part of v1 and will be revisited when external
  loading is designed; they are left in place so the next ADR can
  evolve them deliberately rather than re-discovering them.
- Crate dependencies remain `writ-core`, `serde`, `serde_json`. No
  new crates. `regex` is **not** added: v1 transforms are implemented
  with stdlib string operations (the four chosen are tractable without
  regex). If a future built-in genuinely needs `regex`, that addition
  gets surfaced in the PR that introduces it.

### `writ-core`

- Untouched. The trait operates on `&str` and `String`; no domain
  types cross the boundary. `BufferDocument` is not visible to
  transforms.

### `src-tauri`

- New `commands/transforms.rs` with the two `#[tauri::command]`
  functions. Registered in the existing command-builder list.
- `AppState` gains a `transforms: RwLock<TransformRegistry>` field.
  Initialization in `AppState::initialize` calls
  `writ_plugin::builtins::register_builtins(&mut registry)`.
- A new error mapping helper in `commands/transforms.rs` converts
  `TransformError` and `RegistryError` to `String` for IPC. No
  `?Sized` casts, no `as` boundary tricks.

### Frontend

- `src/services/tauri.ts` gains two typed bindings:
  `listTransforms()` and `applyTransform(id, input)`.
- A new `src/stores/transforms.ts` loads the descriptor list at
  startup and exposes a synchronous reader for the palette.
- `src/commands/registry.ts` is populated at startup with one
  `Command` per registered transform. Each command's `execute()`
  reads the active CodeMirror view, slices the selection text (or
  the full document text if no selection), calls `applyTransform`,
  and dispatches a CodeMirror replacement.
- `src/components/CommandPalette/CommandPalette.tsx` is **not**
  modified — the palette already iterates `getAllCommands()` and the
  transform commands flow through unchanged.
- No new direct `@tauri-apps/api` imports. `tauri.ts` remains the
  single boundary, per ADR-005-aligned frontend rules.

### Settings

- `WritConfig` gains a `transforms: TransformsConfig` section with a
  `disabled: Vec<String>` (default empty). Default-on for all v1
  built-ins. The frontend filters the registry list by this set
  before populating commands.
- The existing settings surface gains a single section listing each
  built-in with a toggle. No redesign of the settings UI; toggles
  are appended to the existing layout.

### Testing

- Unit tests in `writ-plugin`: registry insert/duplicate-id/lookup;
  per-transform round-trip and edge cases (empty input, all-whitespace,
  mixed line endings, Unicode boundaries for the quotes transform,
  tab/space mixed indentation for dedent).
- Integration tests in `src-tauri` for both commands: list returns
  every registered built-in, apply round-trips the expected output,
  unknown id returns a typed IPC error, disabled-transform toggle is
  honored.
- Frontend: store population test (mock `tauri.ts`), command-registry
  population test, and a selection-vs-full-buffer execution test
  against a mocked CodeMirror view.

### Performance

- Each transform must run synchronously and complete in under 16ms on
  a 100KB input on a developer-class laptop (one frame at 60Hz). v1
  transforms are O(n) over the input; this budget is comfortable but
  is asserted with a benchmark test in `writ-plugin` to catch
  accidental quadratic regressions in future built-ins.

## Open questions deferred to follow-up ADRs

- **External plugin loading** (WASM, JS, dynlib): manifest schema
  evolution, loader trust model, sandboxing, capability declarations,
  versioning of the host trait surface. Separate ADR. The registry
  shape decided here is the integration point for that ADR; this
  decision is the contract that future ADR is allowed to extend but
  not break.
- **Async / streaming transforms**: required for any transform that
  hits the network, the model layer, or large-buffer batched work.
  Will need a sibling trait (`AsyncTextTransform`) and a separate IPC
  command that returns a stream / promise rather than a blocking
  result. Not v1.
- **Multi-buffer / workspace-wide transforms**: would change the trait
  signature to take a structured input (slice list, paths, metadata)
  and would need progress reporting and cancellation. Not v1.
- **AI rewriter**: explicitly **not** a `TextTransform`. Even if it
  ends up signature-compatible (string in, string out), it has its
  own UX, latency profile, permissions surface, and provider
  configuration story. It will be a built-in command in its own
  module, registered with the frontend command registry directly,
  not through the transform registry.
