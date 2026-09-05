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
  return stripNoteExtension(wikilinkFileName(target));
}

/**
 * The file name inside a `[[…]]` target: the name with the extension it was
 * written with left on.
 *
 * The last segment of what `Create note` sends. The extension is the caller's
 * to remove, and Rust removes exactly one of it before minting a file name, so
 * `[[Note.md]]` makes `Note.md` and `[[Note.markdown.md]]` makes
 * `Note.markdown.md`, which is the file the target resolves to. Stripping on
 * both sides made `Note.md` out of the second one, a file the link that
 * offered it does not reach.
 */
export function wikilinkFileName(target: string): string {
  return splitTarget(target).name;
}

/**
 * The folder-and-name path inside a `[[…]]` target, `/`-separated.
 *
 * What `Create note` sends. A target carrying a folder only resolves to a note
 * whose own folders end the same way, so the note is created where the target
 * says and the folder travels with the name. `.` and `..` are dropped the way
 * `writ_core::notes::links::stored_target` drops them, and Rust sanitises
 * every segment before anything is created.
 */
export function wikilinkTargetPath(target: string): string {
  const { folder, name } = splitTarget(target);
  return [...folder, name].join("/");
}

/** A target split into the folders written before the name and the name. */
function splitTarget(target: string): { folder: string[]; name: string } {
  const withoutAlias = target.split("|", 1)[0];
  const withoutHeading = withoutAlias.split("#", 1)[0];
  const parts = withoutHeading
    .trim()
    .split(/[\\/]/)
    .map((part) => part.trim())
    .filter((part) => part !== "");
  const name = parts.pop() ?? "";
  return { folder: parts.filter((part) => part !== "." && part !== ".."), name };
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
 * The notes-folder-relative path in a preview href, still escaped, or null
 * when the href is not a link to a note.
 *
 * A fragment names a heading inside the note rather than part of its path, so
 * it is split off first: the `#` in a file name is escaped, which is what
 * leaves the raw one unambiguous.
 *
 * The href arrives from the preview frame, which anything in the rendered
 * document can post, so this only says what the string claims to be. Where it
 * lands is decided by joining it onto the notes folder and refusing anything
 * outside, and whether it opens at all is decided by the index.
 */
export function noteLinkPath(href: string): string | null {
  if (!href.startsWith(NOTE_LINK_SCHEME)) return null;
  const path = href.slice(NOTE_LINK_SCHEME.length).split("#", 1)[0];
  return path === "" ? null : path;
}

/**
 * The heading anchor a preview href carries, unescaped, or null when it
 * carries none.
 *
 * `[[Note#Section]]` is rendered as an href whose fragment is the heading's
 * anchor, so this is what the click has where the editor has the heading text
 * the link was written with. Both reach the same line through the index.
 */
export function noteLinkHeading(href: string): string | null {
  if (!href.startsWith(NOTE_LINK_SCHEME)) return null;
  const rest = href.slice(NOTE_LINK_SCHEME.length);
  const hash = rest.indexOf("#");
  if (hash === -1) return null;
  const fragment = rest.slice(hash + 1);
  if (fragment === "") return null;
  try {
    return decodeURIComponent(fragment);
  } catch {
    return fragment;
  }
}
