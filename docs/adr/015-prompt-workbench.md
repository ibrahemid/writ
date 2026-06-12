# ADR-015: Prompt Workbench — Offline Prompt-Native Editing

**Status:** Accepted
**Date:** 2026-06-12

## Context

Writ's positioning is the scratchpad developers keep next to their coding
tools. In practice a large share of what lives in those scratch buffers is
prompt text: instructions destined for an LLM chat window, an agent harness,
or a CLI tool that accepts a system prompt. Today Writ treats that text as
inert prose. Three concrete frictions show up in daily use:

1. **No size signal.** Prompt budgets are denominated in tokens, but the
   editor reports lines and columns. Users paste into an external counter to
   learn whether a prompt fits a context window.
2. **Noisy copies.** Prompt documents accumulate scaffolding that should not
   be pasted: YAML frontmatter carrying metadata for the user (not the
   model), and HTML comments holding notes-to-self. Stripping these by hand
   before every paste is error-prone, and forgetting leaks private notes
   into the prompt.
3. **Manual templating.** Reusable prompts carry `{{placeholder}}` slots.
   Filling them today means find-and-replace per slot, which mutates the
   template and loses the original.

A **prompt document**, to Writ, is an ordinary text buffer with three
recognized conventions layered on top: optional leading YAML frontmatter
(`---` fenced) as out-of-band metadata, HTML comments as author notes, and
`{{identifier}}` placeholders as fill-in slots. There is no new file type,
no mode, and no schema. Any buffer can be treated as a prompt; the
conventions are recognized, never required.

Everything in this ADR operates offline. Writ ships no network stack for
this feature and calls no model APIs.

## Decision

Add three capabilities, all built on pure functions in `writ-core`:

### 1. Token estimate in the status bar

A heuristic estimator, `prompt::estimate_tokens`, blends two well-known
rules of thumb: English prose averages ~4 characters per token and ~1.33
tokens per word; code skews denser per character. The blend is

```
tokens ≈ 0.6 · (chars / 4) + 0.4 · (words · 4/3)
```

weighted toward the character rule because mixed prose/code buffers track
character counts more reliably than word counts.

The status bar shows `≈ N tok` (compact `1.2k` above a thousand) for the
active buffer, debounced ~500ms behind edits, and hidden when the buffer is
empty. The `≈` is load-bearing: the figure is an estimate, presented as one.

**Why heuristic, not exact.** Exact token counts are tokenizer-specific:
cl100k, o200k, and the SentencePiece families all disagree, and each exact
tokenizer is a multi-megabyte vocabulary artifact tied to one vendor's
models. Shipping one would be wrong for every other model; shipping many
contradicts Writ's size discipline; calling a counting API contradicts the
offline constraint. A blended heuristic is honest about what it is.

**Accuracy band.** For English prose, Markdown, and mainstream code, the
blend lands within roughly ±30% of cl100k-family tokenizers, which is
sufficient for the question the status bar answers ("does this fit a 4k /
32k / 128k window?"). The band is asserted in unit tests against
representative fixtures. Out of band by design: CJK and other scripts where
the chars-per-token ratio collapses toward 1; the estimate will undercount
there and we accept that until a script-aware refinement is warranted.

### 2. Copy as prompt

A pure function, `prompt::strip_for_prompt`, produces the paste-ready form
of a buffer:

- Strips leading YAML frontmatter — only when the first line is exactly
  `---` and a closing `---` line exists. Unterminated frontmatter is
  treated as content and preserved.
- Strips HTML comments (`<!-- … -->`, including multi-line) **outside**
  fenced code blocks. Comments inside ``` or ~~~ fences are code samples
  and are preserved verbatim. An unterminated `<!--` is preserved, not
  swallowed.
- Trims trailing whitespace per line and ends the result with exactly one
  final newline (empty results stay empty).

The strip registers through the existing `TextTransform` registry (ADR-006)
as the `prepare_prompt` built-in, so it is also available as an ordinary
buffer transform via the palette. The dedicated **Copy as Prompt** palette
command runs the same transform over the buffer (selection if present) and
places the result on the clipboard **without mutating the buffer** — the
scaffolding stays in the document; only the copy is clean.

### 3. Fill placeholders

Two pure functions in `writ-core`:

- `prompt::scan_placeholders` finds `{{identifier}}` slots, deduplicates,
  and preserves first-occurrence order. Identifiers are Unicode
  alphanumerics/underscore starting with a letter or underscore. Escaped
  `\{{` is ignored, as are unbalanced or malformed braces.
- `prompt::fill_placeholders` replaces every occurrence of each named slot
  with its supplied value. Slots without a supplied value are left intact;
  escaped openers are never touched.

The **Fill Placeholders** palette command scans the buffer (selection if
present), opens a modal listing one input per placeholder, and on confirm
copies the **filled** text to the clipboard. The buffer is never mutated —
the template survives every use.

## Out of scope (explicitly)

- **Network calls.** No usage endpoints, no remote counting, no telemetry.
- **Per-model exact tokenizers.** No bundled vocabularies, no tokenizer
  plugins. If exact counts ever matter, that is a separate ADR with a size
  budget conversation.
- **Prompt execution.** Writ does not send prompts anywhere. There is no
  model integration, no API-key storage, no response surface. Writ is the
  bench, not the wire.

## Consequences

- `writ-core` gains a `prompt` module: three pure, dependency-free
  function groups with exhaustive unit tests. No new crates.
- `writ-plugin` gains one built-in (`prepare_prompt`, delegating to
  `writ-core`), taking the registry from eight to nine transforms.
- `src-tauri` gains three stateless IPC commands (`prompt_estimate_tokens`,
  `prompt_scan_placeholders`, `prompt_fill_placeholders`). The strip path
  reuses the existing `apply_transform` IPC.
- The frontend gains a token-estimate store (debounce policy lives in the
  store, not the component), a status bar item, two palette commands, a
  clipboard service, and a placeholder-fill modal that follows the
  existing dialog patterns (focus trap, `role="dialog"`, token-only
  styling).
- The estimator's band is documented and tested, so a future refinement
  (script-aware weighting, configurable ratios) changes constants and
  fixtures, not contracts.
