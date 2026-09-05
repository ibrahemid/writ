// Singleton state — Writ is single-window.
import * as tauri from "../../services/tauri";
import { onEvent, type UnlistenFn } from "../../services/events";
import { writeClipboardText } from "../../services/clipboard";
import { showToast } from "../../components/Notifications/Toast";
import { resolveWithinRoot } from "../../lib/path";
import { noteLinkHeading, noteLinkPath } from "../../lib/wikilink";
import type { LinkVerdict } from "../../types/link";
import type { BufferDocument } from "../../types/buffer";
import type { LinkResolution, LinkStatus, NoteNameHit } from "../../services/tauri";

export type { LinkResolution, LinkStatus, NoteNameHit };

const FALLBACK_MESSAGE = "Could not open the link.";

// A refused link surfaces the reason Rust gave, so a `javascript:` or `file:`
// destination reads as a rule rather than as a silent failure.
async function openExternal(url: string): Promise<void> {
  try {
    await tauri.openExternalUrl(url);
  } catch (err) {
    showToast(typeof err === "string" && err !== "" ? err : FALLBACK_MESSAGE, "error");
  }
}

async function classify(url: string): Promise<LinkVerdict> {
  return tauri.classifyExternalUrl(url);
}

/** Copies a link destination, reporting failure rather than swallowing it. */
async function copyLink(text: string): Promise<void> {
  try {
    await writeClipboardText(text);
  } catch {
    showToast("Could not copy the link.", "error");
  }
}

// Every resolution the editor has asked for, so painting a viewport full of
// links costs one call per distinct target rather than one per repaint. The
// key holds both halves because the same target written in two notes can name
// two different notes.
const resolutions = new Map<string, LinkResolution>();
const inFlight = new Map<string, Promise<LinkResolution>>();
let subscription: Promise<UnlistenFn> | null = null;
// Bumped every time the cache is dropped. A consumer that remembers which
// targets it has already asked about compares this to know that its record is
// stale, so a link is asked about again after the note it names appears.
let generation = 0;

function cacheKey(fromPath: string, target: string): string {
  return `${fromPath}\u0000${target}`;
}

/** Forgets every resolution and says so through the generation. */
function drop(): void {
  resolutions.clear();
  generation += 1;
}

/**
 * How many times the cache has been dropped.
 *
 * A surface painting links keeps a record of what it has asked about, which is
 * what stops an index that cannot be read from being asked once per keystroke.
 * This is how that record knows to start over.
 */
function resolutionGeneration(): number {
  return generation;
}

// One listener for the whole cache, opened the first time something asks for a
// resolution. A note added, renamed or removed changes what a target names, and
// the event names the note that changed rather than the links it moved, so the
// whole cache is dropped and the next paint asks again.
function subscribe(): Promise<UnlistenFn> {
  subscription ??= onEvent("notes:changed", () => {
    drop();
  });
  return subscription;
}

/**
 * What `target`, written in the note at `fromPath`, points at.
 *
 * A resolution already read comes back from the cache; a target being read is
 * awaited rather than asked about twice. A failed read reports `missing`,
 * which is the honest answer to "does this note exist" from a call that could
 * not find out — and it is not cached, so the next ask tries again.
 */
async function resolveNoteLink(fromPath: string, target: string): Promise<LinkResolution> {
  const key = cacheKey(fromPath, target);
  const held = resolutions.get(key);
  if (held) return held;
  const pending = inFlight.get(key);
  if (pending) return pending;

  void subscribe();
  const read = tauri
    .resolveNoteLink(fromPath, target)
    .then((resolution) => {
      resolutions.set(key, resolution);
      return resolution;
    })
    .catch((): LinkResolution => {
      return { status: "missing", path: null, candidates: [], heading_line: null };
    })
    .finally(() => {
      inFlight.delete(key);
    });
  inFlight.set(key, read);
  return read;
}

/**
 * What is known about `target` right now, without asking.
 *
 * The editor paints synchronously, so this is what a decoration reads; a
 * target nothing has resolved yet reads as `null` and is painted as neither.
 */
function knownNoteLink(fromPath: string, target: string): LinkResolution | null {
  return resolutions.get(cacheKey(fromPath, target)) ?? null;
}

/**
 * The note a preview link points at, as a path on disk, or null when the href
 * is not one, does not land inside `notesRoot`, or names no note the index
 * holds.
 *
 * The href comes from the preview frame, and anything in the rendered document
 * can post one, so the scheme is a claim and not a permission. Two things
 * decide what it gets: the path is joined onto the notes folder and anything
 * that walks out of it is refused, and the index is asked whether the note it
 * claims is a note at all. Only a link the renderer wrote answers both, so raw
 * HTML carrying the scheme reaches the same popover every other link does
 * rather than opening a file on its own.
 *
 * `fromPath` is the note being previewed, which is what the index ranks the
 * answer against; a preview of something with no file resolves nothing.
 */
async function notePathFromPreview(
  href: string,
  notesRoot: string | null,
  fromPath: string | null,
): Promise<string | null> {
  const claim = noteLinkPath(href);
  if (claim === null || notesRoot === null || notesRoot === "") return null;
  if (fromPath === null || fromPath === "") return null;

  const path = resolveWithinRoot(notesRoot, notesRoot, claim);
  if (path === null) return null;

  const resolution = await resolveNoteLink(fromPath, decodeClaim(claim));
  if (resolution.status !== "resolved" || resolution.path === null) return null;
  return path;
}

/**
 * The note a preview link points at and the line to land on, or null when the
 * href does not open anything.
 *
 * The gate is [`notePathFromPreview`] and nothing here widens it: the heading
 * is looked up only once a href has already earned its file, and a fragment
 * naming no heading opens the note at the top, the same as a `[[Note#gone]]`
 * followed in the editor.
 */
async function noteOpenFromPreview(
  href: string,
  notesRoot: string | null,
  fromPath: string | null,
): Promise<{ path: string; headingLine: number | null } | null> {
  const path = await notePathFromPreview(href, notesRoot, fromPath);
  if (path === null) return null;

  const heading = noteLinkHeading(href);
  if (heading === null) return { path, headingLine: null };
  try {
    return { path, headingLine: await tauri.noteHeadingLine(path, heading) };
  } catch {
    return { path, headingLine: null };
  }
}

/** The href's path as it is written on disk. An undecodable claim is its own. */
function decodeClaim(claim: string): string {
  try {
    return decodeURIComponent(claim);
  } catch {
    return claim;
  }
}

/** Note names for a `[[` completion, ranked by the index quick open reads. */
async function noteNameCandidates(query: string, limit?: number): Promise<NoteNameHit[]> {
  try {
    return await tauri.noteNameCandidates(query, limit);
  } catch {
    return [];
  }
}

/**
 * Creates the note a `[[…]]` target names and opens it.
 *
 * The target goes to Rust as written, folder and extension included: Rust
 * sanitises every segment into a legal name, so the note lands where the link
 * says and the link resolves to it afterwards.
 */
async function createNote(target: string): Promise<BufferDocument | null> {
  try {
    const doc = await tauri.newNoteFromLink(target);
    // The note now exists, so every link that named it resolves differently.
    drop();
    return doc;
  } catch (err) {
    showToast(
      typeof err === "string" && err !== "" ? err : "Could not create the note.",
      "error",
    );
    return null;
  }
}

/** Drops what is cached and stops listening. For tests and for a folder move. */
async function reset(): Promise<void> {
  drop();
  inFlight.clear();
  const held = subscription;
  subscription = null;
  if (held) (await held)();
}

export const linkStore = {
  openExternal,
  classify,
  copyLink,
  resolveNoteLink,
  knownNoteLink,
  resolutionGeneration,
  notePathFromPreview,
  noteOpenFromPreview,
  noteNameCandidates,
  createNote,
  reset,
};
