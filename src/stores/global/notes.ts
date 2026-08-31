import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";
import type {
  MoveNotesOutcome,
  NotesFallbackReason,
  NotesFolderInfo,
} from "../../services/tauri";
import { writeClipboardText } from "../../services/clipboard";

// Singleton state — Writ is single-window. The notes folder changes only when
// the user moves it from Settings, which goes through `move` below and
// refreshes both signals.

export type { MoveNotesOutcome, NotesFallbackReason, NotesFolderInfo };

function createNotesStore() {
  const [root, setRoot] = createSignal<string | null>(null);
  const [folder, setFolder] = createSignal<NotesFolderInfo | null>(null);

  async function load(): Promise<void> {
    try {
      setRoot(await api.getNotesRoot());
    } catch {
      setRoot(null);
    }
  }

  // The path plus what to call it, which is what Settings shows. Kept apart
  // from `load` so a launch pays for one call and the settings row pays for
  // its own.
  async function loadFolder(): Promise<NotesFolderInfo | null> {
    const info = await api.getNotesFolder();
    setFolder(info);
    setRoot(info.path);
    return info;
  }

  async function showInFileManager(): Promise<void> {
    return api.showNotesFolderInFinder();
  }

  async function copyPath(): Promise<void> {
    const path = folder()?.path;
    if (!path) return;
    return writeClipboardText(path);
  }

  // Picks a folder and moves the notes into it in one step. A move that
  // collided changed nothing, so the signals are refreshed only when one
  // happened.
  async function move(): Promise<MoveNotesOutcome | null> {
    const outcome = await api.pickNotesFolder();
    if (outcome && outcome.collided.length === 0) await loadFolder();
    return outcome;
  }

  // Whether a path names a file the notes folder holds, so a row can be shown
  // the note actions rather than nothing. Comparison is on the string the
  // backend gave, which is already canonical; the backend re-checks before it
  // acts, so a wrong answer here costs a menu entry, never a wrong file.
  function contains(path: string): boolean {
    const base = root();
    if (!base) return false;
    const separator = base.includes("\\") ? "\\" : "/";
    const prefix = base.endsWith(separator) ? base : base + separator;
    return path.startsWith(prefix);
  }

  return { root, folder, load, loadFolder, showInFileManager, copyPath, move, contains };
}

export const notesStore = createRoot(createNotesStore);
