import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import type { RenamePropagation } from "../../services/tauri";
import { bufferRegistry } from "./buffer-registry";
import { windowRegistry } from "./window-registry";
import { noteName } from "../../lib/note-name";

// Singleton — app-global, like the registry it renames rows in (ADR-009 E3).

/** What `Undo rename` needs to put one rename back. */
export interface RenameUndo {
  /** The note that was renamed. */
  noteId: string;
  /** Where its file is now. */
  path: string;
  /** The name it had before, without its extension. */
  previousName: string;
  /** The files the rename rewrote, and the only ones the undo touches. */
  paths: string[];
}

function createRenameLinksStore() {
  const [undoable, setUndoable] = createSignal<RenameUndo | null>(null);
  const [skipped, setSkipped] = createSignal<string[]>([]);

  /** How many notes link to the note `id` holds, that note itself left out. */
  async function countLinksTo(id: string): Promise<number> {
    const path = sourcePath(id);
    return path === null ? 0 : api.countLinksTo(path);
  }

  /**
   * Renames the note and, when asked, the links that name it.
   *
   * The row is re-read rather than reloaded wholesale: the file's new name is
   * what the tab bar shows and what the next save writes to. A note that has
   * no file yet has nothing linking to it and takes the plain rename, which is
   * where the backend says so.
   */
  async function renameWithLinks(
    id: string,
    title: string,
    updateLinks: boolean,
  ): Promise<RenamePropagation | null> {
    const path = sourcePath(id);
    if (path === null) {
      await bufferRegistry.renameBuffer(id, title);
      return null;
    }
    const previousName = noteName(id);
    const outcome = await api.renameNoteWithLinks(path, title, updateLinks);
    await settle(id, outcome);
    setUndoable(
      outcome.updated === 0 && outcome.renamed_path === path
        ? null
        : {
            noteId: id,
            path: outcome.renamed_path,
            previousName: withoutExtension(previousName),
            paths: outcome.updated_paths,
          },
    );
    return outcome;
  }

  /** Whether there is a rename to put back. */
  function canUndo(): boolean {
    return undoable() !== null;
  }

  /**
   * Puts the last rename back: the note's previous name, and the links the
   * rename rewrote, rewritten the other way.
   *
   * One rename deep. A second undo would have to know which of two renames a
   * file's current text belongs to, and the answer to that lives in the file,
   * not in a stack.
   */
  async function undoRename(): Promise<RenamePropagation | null> {
    const last = undoable();
    if (last === null) return null;
    const outcome = await api.undoRenameWithLinks(
      last.path,
      last.previousName,
      last.paths,
    );
    setUndoable(null);
    await settle(last.noteId, outcome);
    return outcome;
  }

  /** The notes the last rename left as they were, by name. */
  function skippedNames(): string[] {
    return skipped();
  }

  function clearSkipped(): void {
    setSkipped([]);
  }

  /**
   * What happens after the backend answers: the row catches up, the tabs
   * showing a rewritten file re-read it, and the notes left alone are named.
   *
   * A tab with unsaved edits is left holding them. Re-reading it would throw
   * away what the person typed, and its next save runs into the same guard
   * every other changed file does.
   */
  async function settle(id: string, outcome: RenamePropagation): Promise<void> {
    await bufferRegistry.refreshBuffer(id);
    const win = windowRegistry.getActive();
    if (win) {
      for (const path of outcome.updated_paths) {
        const doc = bufferRegistry
          .activeTabs()
          .find((b) => b.source_path === path);
        if (doc && !win.editor.isDirty(doc.id)) {
          win.editor.requestExternalReload(doc.id);
        }
      }
    }
    setSkipped(outcome.skipped.map((file) => fileName(file.path)));
  }

  function sourcePath(id: string): string | null {
    return bufferRegistry.buffers().find((b) => b.id === id)?.source_path ?? null;
  }

  return {
    countLinksTo,
    renameWithLinks,
    canUndo,
    undoRename,
    skippedNames,
    clearSkipped,
  };
}

/** The last component of a path, which is what a note is called. */
function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/** A note's name without the extension a link never writes. */
function withoutExtension(name: string): string {
  return name.replace(/\.(md|markdown)$/i, "");
}

export const renameLinksStore = createRoot(createRenameLinksStore);
export type RenameLinksStore = ReturnType<typeof createRenameLinksStore>;
