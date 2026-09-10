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

/**
 * Reads what is kept for one note and shows its newest text.
 *
 * A note with nothing kept for it is an empty list rather than a failure: a
 * note nobody has saved since Writ started keeping versions has none, and that
 * is the ordinary case rather than a fault.
 */
async function load(notePath: string): Promise<void> {
  setPath(notePath);
  setSelected(null);
  setText("");
  try {
    const kept = await noteVersions(notePath);
    setVersions(kept);
    if (kept.length > 0) await select(kept[0].id);
  } catch {
    setVersions([]);
    logFailure("the versions of this note could not be read");
  }
}

/** Reads one version's text into the panel. */
async function select(id: number): Promise<void> {
  setSelected(id);
  setReading(true);
  try {
    const content = await noteVersionContent(id);
    if (selected() === id) setText(content);
  } catch {
    if (selected() === id) setText("");
    logFailure("this version could not be read");
  } finally {
    setReading(false);
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
  setPath(null);
  setVersions([]);
  setSelected(null);
  setText("");
}

export const noteVersionsStore = {
  path,
  versions,
  selected,
  text,
  reading,
  load,
  select,
  restore,
  copy,
  clear,
};
