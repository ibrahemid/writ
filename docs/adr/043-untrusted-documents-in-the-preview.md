# ADR-043: Untrusted documents in the preview

## Status

Accepted, 2026-10-13. Amends [ADR-011](./011-preview-trust-model.md), the
lean re-scope in [ADR-009](./009-preview-surface.md), and
[ADR-025](./025-external-link-opening.md) on when the bridge runs. Closes
TS-02, TS-03 and DS-03. Carries decision RC-D1/TS-D1.

## Context

ADR-011 was descoped on the premise that the preview renders Writ's own offline
agent output: with the network off there is nothing to exfiltrate to, so one
fixed document CSP plus a single app-level "run scripts" switch (default on) was
held to make exfiltration impossible. Since [ADR-041](./041-writ-is-a-text-app.md)
that premise no longer holds: file associations route any downloaded `.md` or
`.html` into the same preview, and the preview and a user's own note share one
origin. A downloaded document is untrusted.

The no-egress claim rested on `connect-src 'none'`. That directive closes
`fetch`, XHR, `WebSocket` and `sendBeacon`, and the resource directives close
remote images, fonts, styles and media. It does not govern four channels:

1. **Navigation.** The preview iframe carried no `sandbox` attribute, the
   document CSP carried no `sandbox` directive, `navigate-to` is implemented by
   no shipping engine, and Tauri installs no navigation guard. A script could
   set `top.location` to an attacker URL with the note text in the query
   string.
2. **`RTCPeerConnection` STUN.** ICE gathering sends UDP to a `stun:` host that
   no fetch directive governs.
3. **Resource hints.** `<link rel=dns-prefetch>` and `<link rel=preconnect>`
   resolve a host and open a connection without a fetch. WebKit and Chromium
   do this without consulting the CSP. On WebKit a `preconnect` in a document
   with no script running, including one whose `rel` is spelled with a
   character reference, reaches a local sink under the full policy. Chromium's
   speculative preload scanner reads a tag's attributes before the tree
   builder drops a repeated one and acts on the last `rel`, so
   `<link rel=stylesheet rel=preconnect>` connects although the element the
   tree builder makes is a stylesheet link.
4. **Frame navigation.** `frame-src` refuses a frame navigation aimed outside
   the preview, but Chromium's browser layer, which WebView2 ships, opens a
   connection to the target first. An `<iframe src>` or a
   `<meta http-equiv=refresh>` in a document with scripts off, and a script's
   `location.href` in one with scripts on, each make a TCP connection to the
   host they name, and for `https:` a TLS handshake that carries the host
   name, though no request follows. WebKit refuses these before connecting.
   Both engines build an `<iframe srcdoc>` document under `frame-src 'none'`,
   and its markup, entity-encoded inside the attribute, reaches no rewrite: a
   resource hint in it connects on WebKit and Chromium alike.

Separately, the preview did not draw on Windows (TS-03/DS-03): WebView2 serves a
custom scheme only as `http://<scheme>.<host>`, and every preview URL was
hard-coded to the macOS/Linux `writ-preview://` form, so the iframe, its
runtimes and its images were treated as an external scheme and the pane stayed
blank. wry applies its rewrite to a window's first URL only, never to an iframe
`src`.

## Decision

### 1. Scripts are off for every document, opted in per file

`preview.run_scripts` is retired. A document's own scripts run only if the user
opts the file in; the choice is kept on the buffer row (`preview_scripts`,
migration 047) with the digest of the bytes it covers (`preview_scripts_hash`,
migration 048), so it survives closing the tab and relaunching. Off is the
default for every document, new and existing. A config written by an older
build carries `run_scripts = true`, which is ignored rather than honoured, so
no install keeps scripts on for every document. The opt-in is the command
"Preview: Toggle scripts for this file", not a global switch, because a global
"allow all" contradicts "off for every document". Recording a choice for a
buffer the store does not hold is refused. While the preview shows an HTML or
Markdown document, the status bar reads the choice, checked as below,
`Scripts off` or `Scripts on`, and a click on it runs the same command.

The opt-in is given to the document the user had in front of them, which Writ
built from the bytes it last read from the file. It covers those bytes, whose
digest is recorded with it, and the bytes each save from Writ writes, whose
digest replaces the recorded one (`BufferStore::write_source_guarded`, and the
relaunch's restore of journaled text). A save carries the opt-in over only when
the file held covered bytes as the save's guard found them
(`WriteOutcome::found`), so it cannot pass a change it did not make off as
Writ's. Every check of the opt-in, by a render and by the status bar, compares
the recorded digest with the digest of the bytes Writ holds for the file, the
record the open, the editor's read and each save keep (`AppState::disk_state`;
`BufferStore::check_preview_scripts`,
`writ_core::preview::scripts::opt_in_standing`). Bytes another program wrote
reach the preview only through a read: the editor's read of a tab reopened from
History, of a tab restored at launch, or of a tab reloading after a change. So
a file changed while its tab was closed or while Writ was not running loses its
opt-in at the first check after that read, before a render of the new bytes
runs a script. With none of the file's bytes in hand, a check runs nothing and
keeps the opt-in. No check reads the file: a render or a status-bar read of an
opted-in file on a share that stopped answering does not wait on it, and holds
the buffer store only while it reads and writes the row, so a save of another
tab never waits behind it.

A write that does not come from the editor counts as another program's. A
connected program's write reaches an open tab through the watcher, below. The
chat pane's write and a version restored from history lapse the opt-in where
they announce the write to the tab (`commands::chat::announce_note_change`,
`commands::note_history::restore_note_version_for_tab`): an apply to a file
open in a tab saves through the editor's path, which would otherwise carry the
opt-in over it. A move keeps the opt-in, since the bytes did not change.

For an open tab the change is acted on where it is found, before the tab is
told: the watcher's report of a modification (`NoteFiles::note_file_changed`,
asked by both watchers' modification decisions), and a reopen whose read finds
the file changed (`commands::file::resync_open_buffer`, which also serves
Today's note and a generated document). Each turns the row's scripts off and
serves the cached render with them off (`commands::preview::lapse_scripts_opt_in`).
No event-bus subscriber does it. The bus runs its subscribers on the emitting
thread, and a reopen emits the change with the buffer store's lock held, so a
subscriber that took the store would wait on its own thread for good, with
every later save and journal write behind it. No subscriber may take the
store's lock: the app subscribes in one place (`src-tauri/src/bus_subscribers.rs`),
and `src-tauri/tests/event_bus_store_lock_tests.rs` hangs those subscribers on
a state, holds the store and emits one of every event. The frontend, on the
modification it already handles for the change bar, reads the opt-in again and
reloads the preview showing the file by navigating the iframe, so the document
is rendered and served under the opt-in as it stands; each render's verdict
also reaches the status bar. Writ's own writes never reach the watcher path:
the ignore set and the disk state Writ recorded drop them.

The document CSP's `script-src` follows the opt-in, checked at render time and
carried on the cached render. The first-party runtimes are not the document's
own scripts: the bridge, Mermaid and KaTeX are served same-origin under
`_assets/` as external scripts (their init moved out of inline `<script>`
blocks into `_assets/mermaid/init.js` and `_assets/katex/init.js`), so they run
under `script-src 'self'` whether or not the file is opted in. A diagram or
formula draws either way; only the document's own inline and authored scripts
are gated.

### 2. The frame cannot navigate, and a click asks first

The preview iframe carries `sandbox="allow-scripts allow-same-origin"` and the
document CSP the matching `sandbox allow-scripts allow-same-origin`. Without
`allow-top-navigation`, `allow-popups` and `allow-forms`, the frame cannot
navigate the window, open a window or post a form. `allow-same-origin` keeps
the document's own origin, which `'self'`, the bridge's `postMessage` source
check and the bundled fonts need. The app is a different origin, so the frame
cannot lift its own sandbox.

The app CSP's `frame-src` names the preview origins of each platform:
`writ-preview:` in `tauri.conf.json`, and `http://writ-preview.chrome
http://writ-preview.document` in `tauri.windows.conf.json`, which replaces the
policy string on Windows and differs from the base only in that directive. A
frame navigation to any other origin is refused. WebKit refuses it before any
connection; WebView2's browser layer has connected to the target by then
(Context, item 4), so §3 removes the attributes that start one without a
script.

A navigation guard on the main webview (an inline plugin's `on_navigation`)
allows the app origin, the preview scheme and inert `about:` pages, on Windows
the WebView2 forms of the app, IPC, asset and preview origins, and under
`tauri dev` only the exact origin of the dev server (`devUrl`); it refuses
everything else. On macOS and Linux the engine also routes subframe
navigations through it; WebView2 raises the event for the main frame only, so
there `frame-src` is the frame-level block, with the connection limit above.

The bridge reports a click on an HTML link, an image-map `<area>` or an SVG
link (`href` or `xlink:href`) to the app as a request to open and cancels the
navigation, so a click leaves the decision to the app's link confirmation.

Document and chrome responses send `X-DNS-Prefetch-Control: off`.
`navigate-to 'none'` stays in the CSP but is not relied on.

### 3. Resource hints and frame navigations are disabled before a document is served

The handler passes every document it serves through
`writ_core::preview::hints::neutralize_resource_hints`. A `<link>` keeps its
tag only when the tag ends, carries at most 64 attributes and repeats no name,
and its one `rel`, if it has one, names nothing but `stylesheet` or
`alternate`; a `rel` holding a character reference or a NUL is never kept.
Every other link has its tag renamed to `basefont`, its attributes untouched:
one naming `preconnect`, `dns-prefetch`, `prefetch`, `preload`,
`modulepreload`, `prerender`, `icon`, `manifest` or anything unknown, and one
that repeats an attribute. The tree builder places a `basefont` start tag by
every rule it uses for a `link` (both are void and share each insertion mode's
list), so the rest of the document parses as before; the element is hidden as
a `link` is, and no engine fetches, resolves or connects for it. Renaming the
element does not depend on which `rel` a reader takes from a tag that carries
several: the tree builder keeps the first, Chromium's preload scanner the last.

The rewrite reads the text without tracking parser context: every `<link`
followed by a tag-name terminator is treated as a tag, inside a comment, raw
text or an attribute value included. Wherever an engine could read a link tag,
the tag starts with those bytes and its attributes tokenize the same way, so no
context hides one; rewriting the text of an inert context changes nothing an
engine acts on. A scan budget proportional to the document keeps it linear; a
document that exhausts it has every remaining link disabled. The tests check
the output against html5ever's tokenizer started at every `<`, which counts a
link live when its `rel` names a hint or it repeats an attribute, across the
parser contexts that change how a `<` reads and a generated set of attribute
spellings; a tree-builder comparison checks that a renamed link leaves the
rest of the document's tree as it was.

The handler then passes the document through
`writ_core::preview::frames::disarm_frame_navigations`, which renames each
attribute that starts a frame navigation without a script: `src` and `srcdoc`
on every `iframe` and `frame` tag, and `http-equiv` on every `meta` tag unless
its whole value, in plain text and ASCII case-insensitive, is one of five
pragmas that set no navigation and open no connection: `content-type`,
`content-language`, `default-style`, `x-ua-compatible` and
`content-security-policy`. `refresh` is the pragma that navigates; keeping
only known-inert pragmas, as the hint rewrite keeps only stylesheet links,
also covers any other an engine acts on, such as a `Link` pragma naming a
preconnect, which neither engine acts on today. A value with padding or a
character reference, an empty or missing value, and one too far ahead to read
are renamed. `data-writ-blocked-` goes in front of the name. Letters and
hyphens keep every tokenizer state the name passes through, so the tree is the
same apart from the attribute's name, and every copy of a repeated attribute
is renamed, so no reader finds one whichever copy it takes. A name right after
`</` is also an end tag's name wherever the parser reads the bytes as markup.
It keeps its bytes when, read as an attribute, it has no value, since an empty
`src` loads `about:blank`, an empty `srcdoc` an empty document and an empty
`http-equiv` names no pragma; one that an `=` follows has a value and is
renamed. The tree therefore changes only at an end tag with an `=` after its
name, which is malformed markup. The element keeps its name: an `iframe`'s
content is raw text, and another name would parse its fallback markup. The
document's `frame-src 'none'` already refuses every frame it could show, so
nothing a document displays depends on these attributes, and the rewrite
applies whether or not the file is opted in.

The frame rewrite reads the text without parser context, as the hint rewrite
does: every `<iframe`, `<frame` or `<meta` followed by a tag-name terminator is
a tag. It runs the attribute walks of all such tags together, one byte at a
time, and walks that reach the same state at the same byte go on as one, so
the scan is linear with no budget and no tag is left unread. Both rewrites read
attributes through one model of the tokenizer's attribute states
(`crates/writ-core/src/preview/attributes.rs`). The tests check the output against
html5ever's tokenizer started at every `<`, which counts a frame live while an
`iframe` or `frame` keeps a `src` or `srcdoc` and a refresh live while a
`meta` keeps an `http-equiv` that is not one of the five, across the same
parser contexts and a generated set of attribute spellings; a tree-builder
comparison checks that the rest of the tree is unchanged, and an end-tag test
that an end tag after a tag left open in text keeps its name.

### 4. One preview URL base, decided by the host

The host answers `preview_url_prefix` with `writ-preview://` on macOS and Linux
and `http://writ-preview.` on Windows. At startup the app asks for it through
`src/services/tauri.ts` and hands the answer to the store
`src/stores/global/preview-url.ts`, which holds it as shared state; the store's
`previewUrl(scope, path)` builds every preview URL from it, and components
reach it through the store. A pane that mounts before the base arrives parks on
`about:blank` and renders when it does; if the host cannot answer, every pane
stays parked and the failure is logged. The frontend never derives the scheme
from `navigator.platform`: a dev build can render the Windows chrome on a Mac,
where the engine is still WebKit. In-document asset URLs (the bridge, Mermaid,
KaTeX and note assets) are root-relative (`/_assets/…`, `/_note-asset/…`), so
they resolve against the document's own origin on every platform.

The document CSP lists `http://writ-preview.document` beside `'self'` and
`writ-preview:` in each subresource directive on Windows only; on macOS and
Linux that origin is an ordinary HTTP host a resolver would look up. The
document and chrome `frame-ancestors` name the app origins of each platform,
because `'self'` would refuse the cross-origin embed on Windows.

### 5. A real engine is the authority

`src-tauri/tests/engine_probe.rs` builds the shipped topology in a real
webview: an app-origin page served under the app CSP the build merges for the
platform, framing the preview document in an iframe with the sandbox attribute
`PreviewPane.tsx` sets, at the URL the host's base gives. The document is
rendered by the HTML renderer and served through the production handler, and
navigations go through the production guard. Every vector aims at its own local
TCP sink, STUN at a UDP sink, and, on a run that routes a test domain to it,
host names at a DNS sink.

- **Scripts off.** Resource hints of every kind (one spelled with a character
  reference, two in a second `rel` after a stylesheet's or an alternate's),
  stylesheets including alternates, `@import`, CSS and inline-style
  `url()`, `@font-face`, images, `srcset`, `<picture>`, media, a poster,
  iframes (one with a repeated `src`, two with a hint or a refresh in their
  `srcdoc` markup), an object, an embed, an SVG image, an image input, meta
  refreshes (one with an encoded `http-equiv`, one with a repeated one), a
  `Link` pragma naming a preconnect, an anchor, a `<base>`, a remote and an
  inline script; with the DNS sink on, two
  hints, an image, an anchor and an iframe by name. Asserted: no sink sees
  anything, the inline script does not run, the bridge does, and the app page
  stays.
- **Scripts on.** The document's script tries `fetch`, `sendBeacon`, a
  WebSocket, an image, `top.location`, self-navigation, link clicks with each
  target, an SVG link, an image-map area, `window.open`, a form submit, a
  frame it creates and an inserted meta refresh. Asserted: none reaches a
  sink, each click reaches the app as a request to open, and the app page
  stays. STUN and a preconnect the script creates are reported, not asserted,
  and on Windows so are the three frame navigations (residual risks).
- **Controls.** The same iframe, at the same origin and under the same sandbox,
  loads a document served without the policy that fires an image, a preconnect
  and STUN at fresh sinks; the image must land or the run fails, since silence
  would then prove nothing. With the DNS sink on, the probe first resolves a
  control name itself and the control document loads an image by name; the run
  fails unless the sink sees both. A page with no policy at `about:blank` fires
  the same set as a diagnostic. On Windows the probe turns off Chromium's
  private and local network checks, which would otherwise stop a loopback sink
  for a reason no remote host meets.

The probe runs as the `engine-probe` CI job on macOS, Windows and Linux. The
hand-written CSP evaluator and its vector list are retired: an evaluator can
only restate the policy. The policy bytes are locked by `preview::csp`'s tests
and the app `frame-src` by `tests/app_csp_tests.rs`.

## Consequences

- A document that is not opted in runs none of its own scripts, starts no
  navigation of the window, of its frame or of a frame it embeds, and makes no
  connection through any vector the probe fires, on each engine the probe
  passes on.
- A document's own frames stay empty, and of its `http-equiv` pragmas only
  the five inert ones apply, in every document; `frame-src 'none'` refused
  those frames already.
- The preview draws on Windows.
- Mermaid and KaTeX draw in every note regardless of the opt-in, because their
  runtimes are first-party.
- A file another program changes loses its opt-in before a render of the new
  bytes runs a script, whether its tab was open or closed or Writ was not
  running, and a preview showing it reloads with scripts off.
- No check of the opt-in reads the file, and no event-bus subscriber takes the
  buffer store's lock.

## Residual risks

- **An opted-in file can reach the network.** Neither the CSP nor the sandbox
  governs `RTCPeerConnection`, a resource hint a script inserts after the
  document is served, or a link click a script hides from the bridge (WebKit
  connects to a `target=_blank` link's host even when the sandbox refuses the
  popup). On WebView2 a frame navigation the script starts (a frame it
  creates, a refresh it inserts, `location.href`) connects to its target
  before `frame-src` refuses it; that reaches no host an inserted hint cannot.
  Opting a file in is a decision to run its code; the command says to turn
  scripts on only for a file the user trusts.
- **The opt-in follows the file through Writ's own saves.** A file edited and
  saved in Writ keeps its opt-in, whatever the edit put in it.
- **A change Writ has not read.** The opt-in is checked against the bytes Writ
  last read from the file or wrote to it. While a change no watcher reported
  sits on disk under a tab still showing the text from before it, the preview
  runs the scripts of that text, the document the opt-in was given to; the
  opt-in lapses when Writ reads the change.
- **First-party runtimes as gadgets.** With scripts off any document can still
  load the bridge, Mermaid and KaTeX through `script-src 'self'`. Mermaid runs
  in strict mode and KaTeX with `trust` off, and neither opens a channel the
  policy leaves shut.
- **A person's own click** is not something the probe can make; its scripted
  clicks stand in, and a mouse run on each platform is a manual check.
- **Engine CSP- or sandbox-enforcement bugs** remain out of scope, as in
  ADR-011: a 0-day that bypasses the policy defeats the boundary. The
  mitigation is OS sandboxing and timely engine upgrades; the three-engine
  probe is the standing check that the policy is enforced as written.
