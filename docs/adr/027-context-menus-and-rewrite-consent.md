# ADR-027: Writ owns every context menu, and rewrite consent is asked at first send

## Status

Accepted.

## Context

Two problems surfaced together, and the fixes touch the same surfaces.

**The native menu.** Right-clicking inside the editor produced the webview's own
menu: Look Up, Translate, Speech, Services, Reload. None of it belongs to a text
editor, and none of it does anything a Writ user asked for. Writ already had a
menu component (`components/ContextMenu`), but only the tab bar and three
sidebar lists used it; the editor surface had no `contextmenu` handler at all.

That component also had a latent defect. `MenuItem.separator` means "draw a
divider above this row", and the renderer treats it that way, but `activate()`
and `focusableIndices()` read it as "this row *is* a divider" and refused to run
it. Three shipped menu items were therefore dead on click and unreachable by
keyboard: **Spelling settings**, **Close All Tabs**, and **Clear All History**.

**Rewrite consent.** A hosted rewrite is refused until its host appears in
`ai.consented_hosts`. Consent could only be given from a notice rendered last in
the AI settings section, below the API-key and connection rows. A user who
configured a provider and a key, but never scrolled to that notice, got
`Confirm sending text to this provider first.` on every rewrite — an error that
named no host and offered no way forward. This was the reported failure, and the
config on the reporting machine (`consented_hosts = []` against a DeepSeek base
URL) is exactly that state.

Consent also had two writers. The frontend parsed the base URL with
`new URL().hostname` to decide whether to show the notice and what to store; the
Rust guard parsed it with `url::host_str()` to decide what to check. They agree
today, but nothing held them together.

## Decision

### 1. One endpoint authority

`writ_core::polish::resolve_endpoint` turns a configured `base_url` into an
`EndpointTarget { host, host_port, is_hosted, is_allowed }`. The rewrite guard,
the consent recorder, and the connection probe all call it. `url` was already a
`writ-core` dependency, so the parse moved into the pure crate.

The frontend no longer parses base URLs at all. Two commands expose what it
needs: `ai_endpoint_state` (where the endpoint points, whether it is consented
to, whether a key is set) and `ai_consent_host` (resolve the host and record
it). Because the host is resolved host-side, the string written to
`consented_hosts` is by construction the string the guard later checks.

This is an architecture change, not a bug fix: no host-string disagreement was
reproducible. It removes the class of defect, and moves policy out of a
component.

`prepare_request` now returns a typed `PolishError` — `ConsentRequired { host }`,
`ApiKeyRequired { host }`, `EndpointNotAllowed`, `ModelRequired`, `Disabled` —
instead of hand-written strings, per the crate rules. The error text lives on the
variant, beside the policy that produces it.

### 2. Consent is asked where the work happens

Before any text is sent, `runRewriteAction` resolves every blocker in one pass:
a disallowed URL, missing consent, and a missing key are each surfaced with an
action, and consent and key are handled together so a user who has neither is
never stopped twice.

For an unconsented hosted endpoint the user gets a dialog naming the host — "Send
text to api.deepseek.com?" — and accepting records consent through
`ai_consent_host` before the request runs.

**This does not weaken the privacy posture.** Consent is still explicit,
still per-host, still an affirmative click that names the destination, still
persisted, and still revocable by editing `consented_hosts`. The Rust guard is
unchanged in strictness: it continues to refuse an unconsented hosted request.
The dialog is a way to satisfy the guard, not a way around it. What changed is
that the moment of asking moved from a settings screen the user may never open
to the moment the question is actually live — which is also when the user has
the context to answer it.

The settings notice remains as the second surface, moved above the model and key
rows and now naming the host.

### 3. Writ owns every context menu

The native menu is suppressed app-wide by a single document-level handler that
calls `preventDefault()` and **never** `stopPropagation()`. SolidJS delegates
`contextmenu` at the document (`DelegatedEvents` includes it), so the tab bar and
the three sidebar menus are dispatched through that same listener; stopping
propagation would kill all four. A surface that has already opened its own menu
marks the event as prevented, and the suppressor leaves it alone.

The editor menu is a CodeMirror `domEventHandlers` extension rather than a
document listener: it needs `posAtCoords` to know what is under the pointer, and
it keeps the editor out of the app-wide listener path. Its contents are built by
`buildEditorMenuItems`, a pure function over `{hasSelection, spelling, link,
aiEnabled, hasPlaceholders, editable}`, so the policy is testable without a DOM.

The menu is **flat**, with separator-delimited groups. `MenuItem` has no
children, and building submenu machinery would serve nothing else in the app.

With a selection: spelling corrections (when on a flagged word), link actions
(when on a link), Cut/Copy/Paste, the five rewrite actions (when the feature is
on), and a workspace search seeded with the selection. Without a selection:
Paste and Select all, and no rewrite group — the whole-document path stays on the
palette and the status-bar chip rather than putting five rows in the menu that
all open the same confirmation.

Text inputs get their own Cut/Copy/Paste/Select all menu. Without it, suppressing
the native menu would be a regression for every field in the app.

Clipboard access moved to `tauri-plugin-clipboard-manager`. `navigator.clipboard.readText()`
can raise WebKit's own paste-confirmation UI — the exact foreign artefact being
removed — and webkitgtk's clipboard read is unreliable, so the web API would
behave differently on each of the three platforms Writ ships. Cut and paste go
through CodeMirror transactions, so they land in one undo step and respect a
read-only buffer.

**The preview pane is out of scope.** The `writ-preview://` iframe is a separate
document that a suppressor in the app never reaches, so a right-click there still
gets the native menu. Reaching into it means touching the preview bridge, and the
standing constraint on that iframe (any unmount freezes the macOS webview) makes
that a change to make deliberately and on its own, not as a rider here.

### 4. Spelling gains single-word actions

Spelling could fix everything at once (`Fix all`) or silence a word
(`Add to dictionary`), but a user could not accept one correction, and the
alternatives were never shown at the word. `spellingStore.applyOne` and the pure
`computeChosenFix` add that: unlike the batch fixer they ignore `confident` and
the first-suggestion rule, because the user picked this word and this
replacement; the stale-offset guard still applies, so a moved word is never
overwritten.

Corrections are reachable two ways: double-click the flagged word for a popover
anchored above it, and the right-click menu. **Double-click, not single click**:
clicking into a word to place the caret is how text gets edited, and a popover on
every such click would fight the user. Double-click already selects the word, so
nothing is taken away.

## Consequences

- Three dead menu items work. The `separator` fix repairs them all at once.
- The reported rewrite failure is recoverable in place, and the settings notice
  is visible before a user reaches the key row.
- One URL parser in the product instead of three.
- `ContextMenu` gained viewport clamping and optional shortcut hints; it had
  neither, having only ever opened from the sidebar and the tab bar.
- New dependency: `tauri-plugin-clipboard-manager`, with
  `clipboard-manager:allow-read-text` / `allow-write-text` capabilities.
- `Command` gained `keywords`, mirroring the settings index. The rewrite actions
  are also relabelled with a shared `Rewrite:` prefix — the palette matched
  `rewrite` against only the custom action's label, so the other four were
  unfindable by the obvious query and scattered across the alphabetical browse.
- The preview pane's context menu is inconsistent with the rest of the app until
  it is handled separately.
