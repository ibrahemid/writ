# ADR-025: External Link Opening

**Status:** Accepted
**Date:** 2026-07-25

## Context

Writ has never been able to follow a link. A URL in a buffer is text, and a
link in the preview is an anchor the iframe refuses to navigate. Both surfaces
now routinely hold text an agent wrote or a tool fetched, which is exactly why
the feature was left alone: handing a string from a buffer to the operating
system is handing an untrusted string to whatever application claims its
scheme. `javascript:` is the famous one, but the real surface is wider —
`file:`, `ms-msdt:`, and every custom scheme an installed application
registered are all one click away once the shell calls "open this".

So the question is not whether Cmd+click opens a link. It is where the decision
lives, and how few places can reach the operating system.

## Decision 1: one allowlist, in `writ-core`, applied at one command

`writ_core::link::classify_link` is the only place that decides whether a
string may be opened. It accepts `http`, `https`, and `mailto`, and nothing
else. Every other scheme is refused, including ones that look inert: an
unregistered scheme is precisely the mechanism that hands control to an
arbitrary installed application, so an allowlist is the only shape that is
correct for schemes nobody has thought of yet.

The policy is pure Rust with no I/O, so its adversarial cases are ordinary unit
tests rather than something that has to be exercised through a webview.

Three rules in it are worth recording, because each exists for a specific
attack:

- **The raw string is judged before parsing.** A control character or
  surrounding whitespace is a refusal, not something to trim. The WHATWG URL
  parser strips tab, newline, and carriage return from its input, so
  `java\nscript:` reaches a naive scheme check as a well-formed
  `javascript:` URL. Refusing the raw form keeps the string the user sees and
  the string that is judged identical.
- **The value handed to the operating system is `Url::as_str`**, never the
  caller's input. Whatever the OS receives has been through the parser that
  approved it.
- **`http`/`https` must carry no username or password.**
  `https://bank.example.com@evil.example/` reads as a bank link and resolves to
  an attacker's host.

Refusals carry a `RejectReason`, not the URL. The reason is logged; the URL
never is.

## Decision 2: `tauri-plugin-opener` rather than extending the spawn pattern

Writ already shells out to the platform file manager in
`commands/storage.rs` (`reveal_storage_path`), and extending that pattern to
links would have avoided a dependency. It is the wrong trade.

`reveal_storage_path` passes a path Writ itself constructed, to a fixed set of
arguments, for a directory the user already owns. A link is attacker-influenced
input, and building a command line from attacker-influenced input means owning
per-platform argument escaping for `open`, `explorer`, and `xdg-open` forever.
`explorer.exe` in particular parses its own command line in ways that have
produced a long tail of injection bugs. That is a class of defect not worth
re-owning for a feature this small, and the plugin is maintained by the same
project as the runtime.

## Decision 3: the plugin is initialized but not granted to JavaScript

`tauri_plugin_opener::init()` is registered, and `opener:allow-open-url` is
deliberately **absent** from `capabilities/default.json`. The plugin exists for
the Rust-side `OpenerExt` only.

Without this, the frontend could invoke `plugin:opener|open_url` directly and
the allowlist would be advisory. With it, `open_external_url` is the only path
from the application to an external handler, and `classify_link` runs on the
way through. The absence is asserted by a test
(`src-tauri/tests/link_ipc_tests.rs`), because an ungranted permission is
invisible in review and one line away from being granted by someone wiring up
an unrelated feature.

## Decision 4: file destinations are contained, not opened

A markdown destination without a scheme is a file reference, and it opens in
Writ rather than in the browser. That routing is also a security boundary:
`[read me](../../../.ssh/id_rsa)` is otherwise one click from being read into a
buffer.

`resolveWithinRoot` (`src/lib/path.ts`) drops a fragment or query, decodes
percent-escapes, collapses `.` and `..`, and only then tests containment
against the workspace root with a separator-terminated prefix, so
`<root>-backup` is not inside `<root>`. Anything that resolves outside is
inert — no toast, because a bare word that looks path-like is not an error the
user made. The decode happens before normalization, so `%2e%2e` is collapsed
like any other `..`.

This check runs in the frontend rather than in Rust. The asymmetry is
deliberate: the worst outcome of a miss is a file rendered as text in the
editor, whereas the worst outcome of a link miss is another application
launching, and it reuses `open_file`, which enforces its own limits.

## Decision 5: preview links get a shell-rendered confirmation

The editor can trust its own click. The preview cannot, and no amount of
protocol design fixes that.

The preview is a cross-origin `writ-preview://` iframe, so the shell never sees
pointer or key events inside it — the same constraint that already forces
`bridge.js` to relay zoom keys. A gesture attestation, where the shell stamps a
real user gesture, is therefore not merely awkward but unimplementable: the
shell has nothing to stamp. It would be dead for legitimate users while proving
nothing about forged clicks.

A capability token does not close it either. `bridge.js` is injected at the
body tail, after author scripts, and shares the frame's realm with them. A
`MessagePort` transferred into the frame is delivered to every listener
registered at that moment, so a script that registered first captures it too.
Any signal originating inside the frame is forgeable, including "the user
really clicked".

The property that does hold: **the only trustworthy gesture is one performed in
the shell's own DOM.** A preview link click posts a `link:open` message, and the
shell answers with a small popover showing the classified destination and an
Open button. The user's click on that button is the gesture, and it is the only
caller of `open_external_url` from that path. A forged message produces, at
worst, a popover the user dismisses.

The cost is one extra click per preview link, stated plainly. The alternative
that buys the click back is intercepting navigation at the webview level in
Rust, trusting no JavaScript anywhere; subframe navigation interception is
platform-dependent behavior across WKWebView, WebView2, and WebKitGTK, and it is
not worth the risk for one click. It stays on the table if preview links become
a heavily used path.

With the preview scripts switch off, `bridge.js` does not run, so preview links
stay inert exactly as they are today. That is documented behavior, not a gap.

## Consequences

- `writ-core` gains a `url` dependency. It stays framework-free.
- The editor decorates links only while the modifier is held, so nothing moves
  under the pointer during ordinary editing and the syntax colors stay
  authoritative.
- Modifier-click on a link consumes the event and adds no cursor. Everywhere
  else, CodeMirror's own add-cursor gesture is untouched.
- The link layer is disabled for restricted and large buffers, alongside the
  other content extensions.
- The click reads its modifier from the event, not from the styling state
  field. The field is stale whenever the modifier went down while the editor
  was unfocused, which is the common case for a click that arrives from another
  window.
- Adding a scheme to the allowlist is a one-line change in one file with its
  own tests. That is the intended shape: widening the policy should be visible.
