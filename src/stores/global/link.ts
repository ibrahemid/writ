// Singleton state — Writ is single-window. What a link resolves to is a read
// of the notes index and lives with the others in `index-cache.ts`; this
// store opens, copies, classifies and creates what links point at.
import * as tauri from "../../services/tauri";
import { writeClipboardText } from "../../services/tauri";
import { indexCacheStore } from "./index-cache";
import { showToast } from "./notifications";
import { resolveWithinRoot } from "../../lib/path";
import { noteLinkHeading, noteLinkPath } from "../../lib/wikilink";
import type { LinkVerdict } from "../../types/link";
import type { BufferDocument } from "../../types/buffer";
import type {
  LinkResolution,
  LinkStatus,
  NoteFolderHit,
  NoteNameHit,
} from "../../services/tauri";

export type { LinkResolution, LinkStatus, NoteFolderHit, NoteNameHit };

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

const resolveNoteLink = indexCacheStore.resolveNoteLink;
const knownNoteLink = indexCacheStore.knownNoteLink;
const resolutionGeneration = indexCacheStore.resolutionGeneration;

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

/** The folders a query names, for the rows an `@` offers beside the notes. */
async function noteFolderCandidates(query: string, limit?: number): Promise<NoteFolderHit[]> {
  try {
    return await tauri.noteFolderCandidates(query, limit);
  } catch {
    return [];
  }
}

/** The notes one folder holds, the subfolders included, as the index has them.
 *
 * What attaching a folder attaches. A folder the index cannot answer for
 * attaches nothing rather than some of it. */
async function notePathsInFolder(folder: string): Promise<string[]> {
  try {
    return await tauri.notePathsInFolder(folder);
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
    // The note now exists, so every link that named it resolves differently,
    // and the editor asks again before the index event arrives.
    indexCacheStore.forgetResolutions();
    return doc;
  } catch (err) {
    showToast(
      typeof err === "string" && err !== "" ? err : "Could not create the file.",
      "error",
    );
    return null;
  }
}

/**
 * Image bytes for one reference written in a file, for the editor to draw.
 *
 * A pass-through: the refusal and the size cap are the host's, and the
 * caller decides what a refusal looks like.
 */
async function inlineImage(
  notePath: string | null,
  reference: string,
): Promise<{ dataUrl: string }> {
  return tauri.previewInlineImage(notePath, reference);
}

export const linkStore = {
  openExternal,
  inlineImage,
  classify,
  copyLink,
  resolveNoteLink,
  knownNoteLink,
  resolutionGeneration,
  notePathFromPreview,
  noteOpenFromPreview,
  noteNameCandidates,
  noteFolderCandidates,
  notePathsInFolder,
  createNote,
};
