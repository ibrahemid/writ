# ADR-012: Composite Transforms — Curated Pipelines as the Recipe Seed

**Status:** Accepted
**Date:** 2026-06-06

## Context

ADR-006 shipped the transform runtime: a `TextTransform` trait
(`id` / `metadata` / `apply(&str) -> Result<String>`), a `BTreeMap`
registry behind a `RwLock`, two stateless IPC commands, and four atomic
built-ins (`trim_leading_whitespace`, `normalize_whitespace`,
`smart_to_straight_quotes`, `dedent`). It deliberately deferred two things
to future ADRs: **external plugin loading** (WASM/JS/dynlib, with its
sandbox, trust model, and capability declarations) and any **multi-step or
stateful** transform shape.

Daily use surfaced two concrete gaps the atomic suite cannot close:

1. **No trailing-whitespace trimmer exists.** Leading whitespace has two
   transforms (`trim_leading_whitespace` and `dedent`); trailing whitespace
   has none. The single most common cleanup is missing.
2. **No composition.** Cleaning up arbitrarily-formatted pasted text takes
   three or four separate palette invocations. There is no "format this
   buffer in one command" primitive.

The open product question was whether to answer this by building the
external plugin runtime now, by adding more atomic transforms, or by
introducing user-definable command groups. External loading is a
scale-phase epic with no current demand and a large security surface; it
stays deferred (ADR-006 already owns that boundary). The need today is a
way to **group existing atomic operations into a single command**.

## Decision

Introduce **`CompositeTransform`**: a `TextTransform` that owns an ordered
`Vec<Box<dyn TextTransform>>` and, on `apply`, folds its input through each
sub-transform in order, short-circuiting on the first `TransformError`.

```rust
pub struct CompositeTransform {
    id: String,
    metadata: TransformMetadata,
    steps: Vec<Box<dyn TextTransform>>,
}

impl TextTransform for CompositeTransform {
    fn id(&self) -> &str { &self.id }
    fn metadata(&self) -> &TransformMetadata { &self.metadata }
    fn apply(&self, input: &str) -> Result<String, TransformError> {
        let mut current = input.to_string();
        for step in &self.steps {
            current = step.apply(&current)?;
        }
        Ok(current)
    }
}
```

A composite **is** a `TextTransform`. It registers through the existing
`register`, lists through the existing `list`, and applies through the
existing `apply_transform` IPC command. The trait, the registry surface,
and the IPC contract are **unchanged**. This is an application of ADR-006's
contract, not an extension of it.

Three atomic transforms are added to make the suite complete and to give
the composite its building blocks:

- `trim_trailing_whitespace` — strips trailing spaces/tabs per line.
- `ensure_final_newline` — guarantees exactly one trailing line ending,
  collapsing extra trailing newlines; CRLF-aware; empty stays empty.
- `fix_punctuation_spacing` — removes stray whitespace **before**
  `, . ; : ! ?`, and only when the mark is followed by a boundary
  (whitespace, end of line, end of input, or another such mark). This guard
  protects decimals (`3.14`), URLs (`http://…`), and ellipses from
  corruption.

One curated composite ships:

- `tidy_whitespace` ("Tidy Whitespace") =
  `trim_trailing_whitespace` → `dedent` → `normalize_whitespace` →
  `ensure_final_newline`.

`fix_punctuation_spacing` is intentionally **not** part of `tidy_whitespace`.
Punctuation rewriting is unsafe on buffers that mix prose with code, paths,
URLs, and numbers; folding it into a "format everything" command would make
that command unsafe to run blind. It ships as a separate, opt-in transform
the user invokes deliberately on prose.

## Considered and rejected (for now)

- **External plugin runtime (WASM/JS/dynlib).** Large security surface
  (sandbox, trust model, capabilities, versioning), no current demand.
  Remains deferred to its own ADR per ADR-006. The registry shape is still
  the integration point for that work.
- **User-definable recipes** (`transforms.recipes: [{label, steps:[ids]}]`
  in config plus a settings editor). This is the natural next step, but it
  requires resolving an id list against the registry at apply time, a config
  schema, validation, and settings UI — a distinct feature with its own
  scope, not a larger version of this one. Curated composites ship the
  user-facing win now without that surface.

## Consequences

- The built-in count goes from four to eight (four atomics, one composite).
  `register_builtins` registers all of them; `list()` stays sorted by id.
- `CompositeTransform` is the **seed of user-definable recipes**: when
  recipes land, a recipe loader resolves a stored id list into the same
  `Vec<Box<dyn TextTransform>>` and constructs a `CompositeTransform`. No
  change to the trait, the registry, or the IPC commands is required to get
  there — exactly the loader-agnostic property ADR-006 set out to preserve.
- Composites nest for free (a composite may contain a composite), since a
  composite is itself a `TextTransform`. v1 ships no nested composite, but
  the property holds without special-casing.
- The per-transform performance budget from ADR-006 still applies. A
  composite's cost is the sum of its steps; `tidy_whitespace` is four O(n)
  passes and stays well within the 100KB benchmark budget.
