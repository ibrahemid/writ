# ADR-035: The preview serves files embedded in a note from a route under the document scope

## Status

Accepted. Extends ADR-009; supersedes nothing in it.

## Context

A note that came out of another editor is often half screenshots. Every one of
them rendered as a broken image.

`writ-preview://` had two scopes (`chrome`, `document`), and the document scope
served exactly two things: the HTML the renderer produced for a buffer, and the
bundled host runtimes under `_assets/`, which are bytes compiled into the
binary. Nothing served a file off disk, so `![](attachments/a.png)` had nowhere
to resolve to. ADR-009 kept the read surface at two scopes deliberately, and
widening it is the decision this record covers.

The preview also has a rule inherited from the notes folder: a file a sync
provider has not downloaded must not be read. Reading one materialises it, and
a note full of images would pull down a folder's worth of files during a scroll,
on whatever connection the machine has.

## Decision

**One route, under the existing document scope.**

    writ-preview://document/_note-asset/<buffer id>/<scope token>/<root>/<relative path>

No third scope. `_note-asset` is deliberately distinct from `_assets`, which
serves the host's own runtimes and has different security properties.

**Two containment roots**, both recorded at render time and carried beside the
rendered HTML in the render cache: the notes folder, and the folder holding the
file being previewed. A buffer with no file on disk records neither and serves
no assets at all. `<root>` is a one-character discriminator (`n`, `d`) naming
which one the relative path is expressed against.

**The token binds the URL to one scope.** The buffer id says which scope to
look up; on its own it also let any document reach any other live scope by
naming that buffer's id in a hand-written `<img src>`. The token is minted with
the scope (`AssetScope::for_render`) and appears only in the render that owns
it, so a document reaches its own roots by quoting it back and can quote no
other. A request whose token does not match the scope it names is refused
(`scope_mismatch`) before any path is resolved. The token belongs to the buffer,
not to its path: it is kept across the re-renders of a note and across a move or
a rename of its file, because one that changed would refuse every in-flight
request from the render before it, spraying refusals and security warnings
through ordinary editing. Keeping it across a move grants nothing, since
containment is decided at serve time against the roots the newest render
recorded: the token says which buffer's render emitted the URL, never which path
it may reach.

**Both host-minted segments go into the URL as written**, so the buffer id and
the token must each be one `[A-Za-z0-9_-]` segment. The render-time emitter
checks that and emits no URL at all when either fails, rather than escaping: a
segment carrying `/` or `%` would move where the URL splits into root and
relative path.

**The URL is a claim, not an authorization.** `writ_core::preview::protocol`
decides containment twice over the same rule: once at render time
(`resolve_asset_reference`, which turns an authored reference into a URL) and
again at serve time (`resolve_asset`, which turns the URL back into a path).
Serve time judges the request against the root the URL names, not against both:
a path that leaves that root is refused even where the other root would contain
it.
The rule is the one the workspace root already uses — canonicalise the root,
canonicalise the candidate, require `starts_with` — with two additions:

- A path whose canonical form falls outside both roots is refused and logged.
  It is never served, and the render-time side emits no URL for it, so the
  reference stays as authored and renders as a broken image.
- A symbolic link is refused rather than followed, at either end. The link's
  target is chosen by whoever wrote the link; the containment root is chosen by
  the user. A file reached *through* a linked folder is caught by the same
  canonicalisation, because the resolved path lands outside the root.

The URL parser already rejects `..`, backslash traversal, percent-encoded
traversal, and control characters before any of this runs, so no such path can
reach the resolver. The pure half of the route (`split_asset_request`) is
covered by the existing `preview_url_parser` fuzz target.

**A reference resolves against the note's folder, then the notes folder, then
`attachments/` inside it.** First candidate that exists wins. A leading
separator is read as "from the notes folder", the spelling Obsidian writes.
Three authored forms reach the same resolver: `![](img.png)`, `![[img.png]]`,
and `src` on a raw `<img>`.

**Type by sniffing, never by extension.** The leading bytes decide the
`Content-Type`; a `.png` holding markup sniffs as nothing and is not served.
The extension is what a file's author controls.

**SVG is served as an image, under a policy that cannot run script**:
`default-src 'none'; style-src 'unsafe-inline'; sandbox`, alongside the
`nosniff` every response carries. An SVG loaded through `<img>` is a passive
image and runs no script by the HTML spec; the policy covers the other way in,
a direct navigation to the asset URL. No SVG sanitiser: rewriting a document to
make it safe is a larger and more fragile promise than refusing to give it a
live context.

**Anything that cannot be served is a placeholder naming the file** — a quiet
framed rectangle with the file name in it, served with the same locked policy.
That covers four cases: a file the provider has not downloaded, a missing file,
one past the 16 MB cap, and one whose bytes are not an image. A dataless file is
named without being opened; `is_dataless` reads stat flags only, and the route
returns before any read. A refusal (outside the roots, or a link) is not a
placeholder: it is a 403 with a recorded disposition, because it is a security
event rather than a missing picture.

**Caching is keyed on modification time and size.** Each response carries
`ETag: "<mtime>-<size>"` and `Cache-Control: no-cache`, so an image edited in
place changes the validator and the webview takes the new bytes. Conditional
requests are not implemented: threading request headers into the resolver buys
nothing while every asset is local and small.

**No CSP change.** `build_document_csp` already grants `img-src data:
writ-preview:` (`src-tauri/src/preview/csp.rs:33`), which is exactly what the
route needs. The document policy is untouched by this ADR.

**`writ-render` stays dependency-free.** The rewrite takes an injected
`Option<&dyn Fn(&str) -> Option<String>>`; `src-tauri` supplies the closure and
the wasm entry point supplies none, keeping its signature and the site's output
unchanged. Without a resolver the crate renders every reference form exactly as
it always has.

## Consequences

The preview can read files it could not read before. The containment check and
the traversal rejection are the whole of the safety argument, which is why both
live in `writ-core` beside the protocol fuzz target rather than in the handler.

`resolve` stays free of I/O; the route lives in `resolve_with_assets`, which is
what the Tauri glue calls. A reader editing `resolve` will not accidentally
change what the asset route does.

The `![[img.png]]` form is handled here as the image subset of Obsidian embeds,
in one named function in `writ-render`. The general link policy lands later in
`writ_core::notes::links`, and this subset is expected to route through it then.

`RenderRequest` gained an `assets` field. It is plain data, not a closure, so
the renderer registry keeps holding trait objects and the request type keeps its
serde shape.
