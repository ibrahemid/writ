import { createSignal, createRoot } from "solid-js";
import * as api from "../../services/tauri";

// Singleton state — Writ is single-window. The notes folder is fixed for the
// life of the process, so it is read once and held.

function createNotesStore() {
  const [root, setRoot] = createSignal<string | null>(null);

  async function load(): Promise<void> {
    try {
      setRoot(await api.getNotesRoot());
    } catch {
      setRoot(null);
    }
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

  return { root, load, contains };
}

export const notesStore = createRoot(createNotesStore);
