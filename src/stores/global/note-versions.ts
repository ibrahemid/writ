import { createSignal } from "solid-js";

import {
  copyNoteVersion,
  noteVersionContent,
  noteVersions,
  restoreNoteVersion,
  type NoteVersion,
  type RestoredVersion,
  type VersionCopy,
} from "../../services/tauri";
import { logFailure } from "../../lib/log";

export type { NoteVersion, RestoredVersion, VersionCopy };

// Singleton state — Writ is single-window

const [path, setPath] = createSignal<string | null>(null);
const [versions, setVersions] = createSignal<NoteVersion[]>([]);
const [selected, setSelected] = createSignal<number | null>(null);
const [text, setText] = createSignal("");
const [reading, setReading] = createSignal(false);
const [loading, setLoading] = createSignal(false);

// Every read carries a token. Arrowing down a long list opens one read per
// row, and an earlier one settling must not write the pane or un-blank it
// under a row it no longer belongs to: only the newest token may do either.
let readToken = 0;

/**
 * Reads what is kept for one note and shows its newest text.
 *
 * A note with nothing kept for it is an empty list rather than a failure: a
 * note nobody has saved since Writ started keeping versions has none, and that
 * is the ordinary case rather than a fault.
 */
async function load(notePath: string): Promise<void> {
  // Before anything else: a restore re-reads the list, and the read the old row
  // started is still open across that window. Without this it keeps the winning
  // token and draws the pre-restore text into a pane with no row selected.
  readToken += 1;
  setPath(notePath);
  setSelected(null);
  setText("");
  setLoading(true);
  try {
    const kept = await noteVersions(notePath);
    setVersions(kept);
    if (kept.length > 0) await select(kept[0].id);
  } catch {
    setVersions([]);
    logFailure("the versions of this note could not be read");
  } finally {
    setLoading(false);
  }
}

/** Reads one version's text into the panel. */
async function select(id: number): Promise<void> {
  const token = ++readToken;
  setSelected(id);
  setReading(true);
  try {
    const content = await noteVersionContent(id);
    if (token === readToken) setText(content);
  } catch {
    // Silent unless this is still the read the panel waits for: a failure on a
    // row the user has already arrowed past names a version that is no longer
    // on screen, so the line could not say which one it meant.
    if (token === readToken) {
      setText("");
      logFailure("this version could not be read");
    }
  } finally {
    if (token === readToken) setReading(false);
  }
}

/**
 * Writes one version back to its note, then re-reads the list.
 *
 * The text the note held is kept as the restore lands, so it comes back as a
 * row of its own and going back is another restore.
 */
async function restore(id: number): Promise<RestoredVersion> {
  const restored = await restoreNoteVersion(id);
  const notePath = path();
  if (notePath) await load(notePath);
  return restored;
}

/** Writes one version beside its note, leaving the note alone. */
async function copy(id: number): Promise<VersionCopy> {
  return copyNoteVersion(id);
}

/** Drops what the panel was showing, for a panel that closed. */
function clear(): void {
  readToken += 1;
  setReading(false);
  setPath(null);
  setVersions([]);
  setSelected(null);
  setText("");
  setLoading(false);
}

export const noteVersionsStore = {
  path,
  versions,
  selected,
  text,
  reading,
  loading,
  load,
  select,
  restore,
  copy,
  clear,
};
