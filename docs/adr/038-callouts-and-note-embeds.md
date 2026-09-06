# ADR-038: Callouts and note embeds are rendering policy in `writ-render`

## Status

Accepted. Extends ADR-034 and ADR-035; supersedes nothing.

## Context

A folder written in Obsidian carries two constructs the preview rendered as
the characters they were typed as. A callout — a blockquote whose first line is
`> [!warning]` — rendered as a quotation with a stray bracket in it. A note
embed — `![[Some Note]]` — rendered as that literal text, because
`rewrite_image_embeds` deliberately rewrites only the references that name an
image file and leaves everything else alone.

Both are worth having, and both raise the same question of where they belong.
`writ-render` is shared: the app calls it and so does the site, which compiles
it to wasm and supplies no resolvers. Anything the crate learns to do has to
keep building without Tauri, without the notes index and without a filesystem,
and has to leave a document that uses neither construct rendering exactly the
bytes it rendered before.

An embed also asks a question a link never has to answer. `[[A]]` points at a
note; `![[A]]` shows it. A note that embeds itself, or two that embed each
other, is a cycle, and a cycle rendered naively either never terminates or
takes the stack down with it.

## Decision

**Both are policy in `writ-render`.** Callouts need no host at all: the syntax
is read out of the parser's own event stream in `callout.rs`, and the wrapper
it becomes is the same markup for every caller, the site included. Embeds take
a resolver, `NoteEmbedResolver`, beside the `WikilinkResolver` ADR-034 added,
under the same rule — the crate owns the markup, the host owns the answer.

**A target's states are the link's three, plus two the crate has to be told
about.** `Ambiguous` and `Missing` render a plain span, exactly as the link form
of the same target does; the read model carries ambiguity and never guesses.
`NotDownloaded` is a note the index named and whose bytes are not on this
machine: it renders a placeholder and no read is attempted, so an embed never
makes a sync provider fetch a file the way ADR-028 §5 refuses to. `Cut` is a
note the host chose not to read because the render was going to stop there
anyway.

**The cycle is stopped by the crate, not by the host.** Depth and the set of
notes already being rendered are parameters of the render and of
`NoteEmbedResolver::resolve`. The host gets them so it can skip a read whose
bytes would be thrown away; the crate applies both limits to whatever comes
back regardless, so a resolver that ignores them cannot recurse. Three embeds
deep is the ceiling and the fourth renders as a link. Both limits are function
parameters, so both are tested without a filesystem.

**An unknown callout type keeps its word.** `data-callout` carries the word as
authored and `data-callout-type` carries the type the styling uses, which is
`note` for anything the table does not name. Nothing an author wrote is
dropped, and a stylesheet can reach a type this build has never heard of.

**An embedded note is rendered against its own folder.** The resolver hands
back an `EmbedBase` alongside the note's text: the folder's answers to a file
reference, to a `[[…]]` and to a nested `![[…]]`. The same link in the same
note therefore names the same note wherever it is shown. The base is the outer
render's asset scope with the target's folder in place of the embedding note's
and everything else, the token included, left alone: the token says which
document owns the scope (ADR-035), and an embedded note is part of that
document. Containment does not move with the base — a reference is still
confined to the two roots, and the target's folder came out of the index, so it
is already inside the notes root. A resolver that hands back no base has no
notion of folders, so its note renders with no file and no link resolver rather
than with the embedding note's.

**An embedded note's headings are demoted below the heading it sits under.**
Every heading in the note shifts down by the level of the host heading the
embed was written under, clamped at `h6`, so an embed can never outrank the
note showing it. An embed at the top of a document shifts nothing.

**A resolved embed is lifted out of its paragraph.** The section it renders as
is a block and a paragraph holds phrasing content, so the paragraph is split
around it. A span — missing, ambiguous, or a cut point — stays inline, which
is where the link form of the same target sits.

## Consequences

`render_markdown_fragment_with` takes a fourth parameter. The wasm entry point
passes none of the four, so the site renders a callout (no resolver needed) and
leaves `![[Some Note]]` as the text it was written as.

A callout written closed shows its title and no body, and cannot be opened: the
markup is a `div`, and the preview document has no control that would fold it.

The preview token layer gains `--writ-preview-status-success`, `-warning`,
`-error` and `--writ-preview-r-card`. The iframe cannot read the app root
(`design/tokens/preview.json`), so the app's `--writ-status-*` and
`--writ-r-card` have preview-scope counterparts rather than being reached
across the boundary. Values are the app's, per polarity.

The bundled preview runtimes this unit did not change, recorded here because
nothing else records them: KaTeX 0.16.22 (MIT) and Mermaid 11.15.0 (MIT), both
under `src-tauri/assets/` and served from the document-scope asset route
(ADR-035). Both are declared in `licences/notices.toml` and listed in
`THIRD-PARTY-NOTICES.md`.
