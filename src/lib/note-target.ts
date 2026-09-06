import { basename, dirname } from "./path";

/**
 * What a link's target is called, for a surface listing links as text.
 *
 * A link is written as a name and resolved to a path (ADR-034); these turn a
 * path back into the name a person wrote, and name the notes a target could
 * have meant when it named more than one.
 */

/** What a note is called, ending in a note extension the link never writes. */
const NOTE_EXTENSION = /\.(md|markdown|mdown|mkd|txt)$/i;

/** What a note is called: its file name, without the extension. */
export function targetName(path: string): string {
  return basename(path).replace(NOTE_EXTENSION, "");
}

/**
 * A candidate note, by its folder and its name.
 *
 * The name alone cannot tell an ambiguity apart: the two notes are ambiguous
 * because they are called the same thing. The folder is what separates them,
 * so it is what the marker shows.
 */
export function candidateName(path: string): string {
  const parent = dirname(path);
  const folder = parent === path ? "" : basename(parent);
  const name = targetName(path);
  return folder === "" ? name : `${folder}/${name}`;
}

/** What else the link could mean, named. */
export function ambiguityMarker(candidates: string[]): string {
  const names = candidates.map(candidateName);
  if (names.length === 0) return "Could name another note";
  if (names.length === 1) return `Could also mean ${names[0]}`;
  return `Could also mean ${names.slice(0, -1).join(", ")} or ${names[names.length - 1]}`;
}
