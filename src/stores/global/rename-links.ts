import { createRoot, createSignal } from "solid-js";
import * as api from "../../services/tauri";
import type { RenamePropagation } from "../../services/tauri";
import { bufferRegistry } from "./buffer-registry";
import { windowRegistry } from "./window-registry";
import { notesStore } from "./notes";
import { describeSkippedNote } from "../../lib/save-error";
import { logFailure } from "../../lib/log";
import { reportOpenOutcome } from "./open-report";

// Singleton — app-global, like the registry it renames rows in (ADR-009 E3).

/** A note a rename left as it was, as the bar names it. */
export interface SkippedNote {
  /** The file's name. */
  name: string;
  /** Where the file is, for the row that opens it. */
  path: string;
  /** Why it was left, in words ([`describeSkippedNote`]). */
  reason: string;
  /** The name its stale links still use, or null when it is not known. */
  linkName: string | null;
}

/**
 * The 1-based line of the first `[[name` link in `text`, or null. Compared
 * case-insensitively, the way links resolve.
 */
export function lineOfLinkTo(text: string, name: string): number | null {
  const needle = `[[${name}`.toLowerCase();
  const lines = text.split(/\r?\n/);
  const at = lines.findIndex((line) => line.toLowerCase().includes(needle));
  return at === -1 ? null : at + 1;
}

function stemOf(path: string): string {
  const name = fileName(path);
  const dot = name.lastIndexOf(".");
  return dot > 0 ? name.slice(0, dot) : name;
}

/** What `Undo rename` needs to put one rename back. */
export interface RenameUndo {
  /** The note that was renamed. */
  noteId: string;
  /** What the backend handed out for this rename. The files the undo touches
   * are the ones the rename rewrote, and the backend holds that list. */
  token: string;
}

function createRenameLinksStore() {
  const [undoable, setUndoable] = createSignal<RenameUndo | null>(null);
  const [skipped, setSkipped] = createSignal<SkippedNote[]>([]);

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
   * where the backend says so. A file opened from outside the folder holds
   * no links the index knows of and takes the plain rename too: the backend
   * rewrites links only for files in the folder.
   */
  async function renameWithLinks(
    id: string,
    title: string,
    updateLinks: boolean,
  ): Promise<RenamePropagation | null> {
    const path = sourcePath(id);
    if (path === null || !notesStore.contains(path)) {
      await bufferRegistry.renameBuffer(id, title);
      return null;
    }
    const outcome = await api.renameNoteWithLinks(path, title, updateLinks);
    await settle(id, outcome, stemOf(path));
    setUndoable(outcome.undo_token === null ? null : { noteId: id, token: outcome.undo_token });
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
   * not in a stack. `token`, when given, names the rename to put back: an
   * Undo left on screen from an earlier rename does nothing once a later one
   * has replaced it.
   */
  async function undoRename(token?: string): Promise<RenamePropagation | null> {
    const last = undoable();
    if (last === null) return null;
    if (token !== undefined && token !== last.token) return null;
    const before = sourcePath(last.noteId);
    const outcome = await api.undoRenameWithLinks(last.token);
    setUndoable(null);
    await settle(last.noteId, outcome, before === null ? null : stemOf(before));
    return outcome;
  }

  /** The notes the last rename left as they were, each with its reason. */
  function skippedNotes(): SkippedNote[] {
    return skipped();
  }

  function clearSkipped(): void {
    setSkipped([]);
  }

  /** Opens a note the rename left alone, at its first stale link. */
  async function openSkipped(note: SkippedNote): Promise<void> {
    const win = windowRegistry.getActive();
    if (!win) return;
    const outcome = await win.tabs.openPath(note.path);
    reportOpenOutcome(outcome);
    if (outcome.kind !== "opened" || note.linkName === null) return;
    try {
      const line = lineOfLinkTo(await bufferRegistry.readContent(outcome.doc.id), note.linkName);
      if (line !== null) win.editor.requestReveal(outcome.doc.id, line);
    } catch {
      logFailure("a note a rename left alone could not be read for its link");
    }
  }

  /**
   * What happens after the backend answers: the row catches up, the tabs
   * showing a rewritten file re-read it, and the notes left alone are named.
   *
   * A tab with unsaved edits is left holding them. Re-reading it would throw
   * away what the person typed, and its next save runs into the same guard
   * every other changed file does.
   */
  async function settle(
    id: string,
    outcome: RenamePropagation,
    linkName: string | null,
  ): Promise<void> {
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
    setSkipped(
      outcome.skipped.map((file) => ({
        name: fileName(file.path),
        path: file.path,
        linkName,
        reason: describeSkippedNote(
          file.reason,
          file.other_path ? fileName(file.other_path) : null,
        ),
      })),
    );
  }

  function sourcePath(id: string): string | null {
    return bufferRegistry.buffers().find((b) => b.id === id)?.source_path ?? null;
  }

  return {
    countLinksTo,
    renameWithLinks,
    canUndo,
    undoRename,
    skippedNotes,
    clearSkipped,
    openSkipped,
  };
}

/** The last component of a path, which is what a note is called. */
function fileName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

export const renameLinksStore = createRoot(createRenameLinksStore);
export type RenameLinksStore = ReturnType<typeof createRenameLinksStore>;
