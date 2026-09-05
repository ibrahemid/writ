/**
 * The note name inside a `[[…]]` target.
 *
 * Mirrors `writ_core::notes::links::parse_wikilink`: the alias is what follows
 * the first `|`, the heading is what follows the first `#` before it, the name
 * is the last non-empty `/` or `\` segment, and a note extension on it is
 * removed so `[[Note.md]]` and `[[Note]]` are the same link. What is left is
 * what a surface shows and what `Create note` names a file.
 *
 * The two implementations are pinned against each other by the shared corpus
 * in `crates/writ-core/tests/fixtures/wikilink-targets.json`.
 */
const NOTE_EXTENSIONS = ["md", "markdown"];

export function wikilinkName(target: string): string {
  const withoutAlias = target.split("|", 1)[0];
  const withoutHeading = withoutAlias.split("#", 1)[0];
  const parts = withoutHeading.trim().split(/[\\/]/).filter((part) => part !== "");
  return stripNoteExtension((parts.pop() ?? "").trim());
}

/** `name` without a trailing note extension. */
function stripNoteExtension(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot === -1) return name;
  const extension = name.slice(dot + 1).toLowerCase();
  return NOTE_EXTENSIONS.includes(extension) ? name.slice(0, dot) : name;
}

/**
 * What the preview writes a link to a note with.
 *
 * Kept in step with `NOTE_LINK_SCHEME` in `src-tauri/src/preview/wikilinks.rs`.
 * A note is not a web address, so a preview link to one carries a scheme of
 * its own rather than a bare relative path the external-link policy would
 * refuse as unparseable.
 */
export const NOTE_LINK_SCHEME = "writ-note:";

/**
 * The notes-folder-relative path in a preview href, or null when the href is
 * not a link to a note.
 *
 * The href arrives from the preview frame, which anything in the rendered
 * document can post, so this only says what the string claims to be. Where it
 * lands is decided by joining it onto the notes folder and refusing anything
 * outside.
 */
export function noteLinkPath(href: string): string | null {
  if (!href.startsWith(NOTE_LINK_SCHEME)) return null;
  const path = href.slice(NOTE_LINK_SCHEME.length);
  return path === "" ? null : path;
}
