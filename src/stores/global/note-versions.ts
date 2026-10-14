import { createSignal } from "solid-js";

import {
  compareNoteVersion,
  copyNoteVersion,
  noteVersionContent,
  noteVersions,
  restoreNoteVersion,
  type DiffHunk,
  type NoteVersion,
  type NoVersionsReason,
  type RestoredVersion,
  type VersionCopy,
} from "../../services/tauri";
import { logFailure } from "../../lib/log";

export type { DiffHunk, NoteVersion, NoVersionsReason, RestoredVersion, VersionCopy };

/** A restore the panel can still take back: restoring `id` puts back the text
 * the restore replaced. */
export interface UndoableRestore {
  id: number;
  note: string;
}

// Singleton state — Writ is single-window

const [path, setPath] = createSignal<string | null>(null);
const [versions, setVersions] = createSignal<NoteVersion[]>([]);
const [reason, setReason] = createSignal<NoVersionsReason | null>(null);
const [failed, setFailed] = createSignal(false);
const [selected, setSelected] = createSignal<number | null>(null);
const [text, setText] = createSignal("");
const [textError, setTextError] = createSignal<string | null>(null);
const [reading, setReading] = createSignal(false);
const [loading, setLoading] = createSignal(false);
const [comparing, setComparing] = createSignal(false);
// `null` until the comparison for the selected version has arrived.
const [hunks, setHunks] = createSignal<DiffHunk[] | null>(null);
const [compareError, setCompareError] = createSignal<string | null>(null);
const [undo, setUndo] = createSignal<UndoableRestore | null>(null);

// Every read carries a token. Arrowing down a long list opens one read per
// row, and an earlier one settling must not write the pane or un-blank it
// under a row it no longer belongs to: only the newest token may do either.
let readToken = 0;

function messageOf(error: unknown, fallback: string): string {
  if (typeof error === "string") return error;
  if (error instanceof Error && error.message) return error.message;
  return fallback;
}

/**
 * Reads what is kept for one note and shows its newest text.
 *
 * A note with nothing kept for it is an empty list with the reason Rust gives
 * for it. A list that could not be read is a failure of its own, never the
 * empty list: a person looking for lost text has to know to try again.
 */
async function load(notePath: string): Promise<void> {
  // Before anything else: a restore re-reads the list, and the read the old row
  // started is still open across that window. Without this it keeps the winning
  // token and draws the pre-restore text into a pane with no row selected.
  readToken += 1;
  setPath(notePath);
  setSelected(null);
  setText("");
  setTextError(null);
  setHunks(null);
  setCompareError(null);
  setFailed(false);
  setReason(null);
  setUndo(null);
  setLoading(true);
  try {
    const kept = await noteVersions(notePath);
    setVersions(kept.versions);
    setReason(kept.no_versions_reason);
    if (kept.versions.length > 0) await select(kept.versions[0].id);
  } catch {
    setVersions([]);
    setFailed(true);
    logFailure("the versions of this note could not be read");
  } finally {
    setLoading(false);
  }
}

/** Reads the list again, for a panel whose last read failed. */
async function retry(): Promise<void> {
  const notePath = path();
  if (notePath) await load(notePath);
}

/** Reads one version's text into the panel, and its comparison when the panel
 * is comparing. */
async function select(id: number): Promise<void> {
  const token = ++readToken;
  setSelected(id);
  setReading(true);
  setTextError(null);
  setHunks(null);
  setCompareError(null);
  try {
    const content = await noteVersionContent(id);
    if (token === readToken) setText(content);
  } catch (error) {
    // Silent unless this is still the read the panel waits for: a failure on a
    // row the user has already arrowed past names a version that is no longer
    // on screen, so the line could not say which one it meant.
    if (token === readToken) {
      setText("");
      setTextError(messageOf(error, "That version could not be read."));
      logFailure("this version could not be read");
    }
  } finally {
    if (token === readToken) setReading(false);
  }
  if (token === readToken && comparing()) await loadComparison(id, token);
}

async function loadComparison(id: number, token: number): Promise<void> {
  try {
    const found = await compareNoteVersion(id);
    if (token === readToken) setHunks(found);
  } catch (error) {
    if (token === readToken) {
      setHunks(null);
      setCompareError(messageOf(error, "That version could not be compared."));
    }
  }
}

/** Shows the selected version as the changes restoring it would make, or as
 * its plain text. */
async function setCompare(on: boolean): Promise<void> {
  setComparing(on);
  setHunks(null);
  setCompareError(null);
  const id = selected();
  if (on && id !== null) await loadComparison(id, readToken);
}

/**
 * Writes one version back to its note, then re-reads the list.
 *
 * The text the note held is kept as the restore lands, and the restore names
 * the version holding it, which is what Undo puts back.
 */
async function restore(id: number): Promise<RestoredVersion> {
  const restored = await restoreNoteVersion(id);
  const notePath = path();
  if (notePath) await load(notePath);
  setUndo(restored.replaced === null ? null : { id: restored.replaced, note: restored.note });
  return restored;
}

/** Takes the last restore back by restoring the text it replaced. */
async function undoRestore(): Promise<RestoredVersion | null> {
  const target = undo();
  if (!target) return null;
  setUndo(null);
  const restored = await restoreNoteVersion(target.id);
  const notePath = path();
  if (notePath) await load(notePath);
  return restored;
}

/** Writes one version beside its note as a new file, leaving the note alone. */
async function copy(id: number): Promise<VersionCopy> {
  return copyNoteVersion(id);
}

/** Drops what the panel was showing, for a panel that closed. */
function clear(): void {
  readToken += 1;
  setReading(false);
  setPath(null);
  setVersions([]);
  setReason(null);
  setFailed(false);
  setSelected(null);
  setText("");
  setTextError(null);
  setLoading(false);
  setComparing(false);
  setHunks(null);
  setCompareError(null);
  setUndo(null);
}

export const noteVersionsStore = {
  path,
  versions,
  reason,
  failed,
  selected,
  text,
  textError,
  reading,
  loading,
  comparing,
  hunks,
  compareError,
  undo,
  load,
  retry,
  select,
  setCompare,
  restore,
  undoRestore,
  copy,
  clear,
};
