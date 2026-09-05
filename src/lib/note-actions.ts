import { bufferRegistry } from "../stores/global/buffer-registry";
import { notesStore } from "../stores/global/notes";
import { windowRegistry } from "../stores/global/window-registry";
import { requestChoice, requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { renameLinksStore } from "../stores/global/rename-links";
import { linkCountQuestion } from "./rename-copy";
import { showToast } from "../components/Notifications/Toast";
import { logFailure } from "./log";
import { noteName } from "./note-name";
import { detectPlatform } from "./platform";
import * as api from "../services/tauri";

export { noteName };

// The note operations three surfaces share: the command palette, the tab
// context menu and the file tree. Keeping them here is what stops the same
// confirmation being worded three ways.


/**
 * Renames a note, offering first to update the notes that link to it.
 *
 * The offer is made only when there are links to update, and it names how many
 * there are: a rename that silently rewrites other people's notes, and one
 * that silently leaves every link pointing at a name no note answers to, are
 * both worse than being asked. Each answer is a button: one renames and
 * rewrites, the other renames and leaves the links. Escape and a click outside
 * are neither, and stop the rename — a keypress that means "get me out of
 * this" cannot be the one that breaks three notes' links.
 *
 * Errors are the caller's: a rename can be stopped by an empty name, a name
 * the folder already holds, or a file something else rewrote, and the surface
 * that took the typing is where that has to be said. So can the count, which
 * is read before anything is renamed: not knowing whether other notes point
 * here is not the same as knowing that none do.
 */
export async function renameNoteAndLinks(id: string, title: string): Promise<void> {
  let updateLinks = false;
  const count = await renameLinksStore.countLinksTo(id);
  if (count > 0) {
    const answer = await requestChoice({
      title: linkCountQuestion(count),
      message: "Links in other notes are rewritten to the new name.",
      confirmLabel: "Update links",
      cancelLabel: "Rename only",
    });
    if (answer === "dismissed") return;
    updateLinks = answer === "confirm";
  }
  await renameLinksStore.renameWithLinks(id, title, updateLinks);
}

/**
 * Whether Writ may move this note to the Trash.
 *
 * A tab can hold a file opened from anywhere, and a Delete on it has to mean
 * "delete my note", never "delete somebody's file". The backend decides the
 * same way and stops the call regardless; this is what keeps the entry from
 * being offered at all. A note that never reached a file has nothing outside
 * the folder.
 */
export function noteIsDeletable(id: string): boolean {
  const doc = bufferRegistry.buffers().find((b) => b.id === id);
  if (!doc) return false;
  if (!doc.source_path) return true;
  return notesStore.contains(doc.source_path);
}

/** What the platform calls its file manager. */
export function showInFileManagerLabel(): string {
  switch (detectPlatform()) {
    case "mac":
      return "Show in Finder";
    case "win":
      return "Show in Explorer";
    default:
      return "Show in Files";
  }
}

/**
 * Asks, then moves the note to the Trash and closes its tab.
 *
 * The confirmation is not ceremony: the note leaves Writ entirely, and getting
 * it back means going to the Trash for it.
 */
export async function confirmAndDeleteNote(id: string): Promise<void> {
  const name = noteName(id);
  if (!noteIsDeletable(id)) {
    showToast("Only notes in your notes folder can be moved to the Trash from here.", "error");
    return;
  }
  const confirmed = await requestConfirm({
    title: `Move "${name}" to the Trash?`,
    message: "You can get it back from the Trash.",
    confirmLabel: "Move to Trash",
    danger: true,
  });
  if (!confirmed) return;

  const win = windowRegistry.getActive();
  try {
    if (win) {
      await win.tabs.deleteNote(id);
    } else {
      await bufferRegistry.deleteNote(id);
    }
  } catch {
    logFailure("a note could not be moved to the trash");
    showToast(`Couldn't move "${name}" to the Trash.`, "error");
  }
}

/**
 * Writes a copy of the note into the notes folder and opens it.
 *
 * The text comes from the editor rather than the file, so a copy taken mid-edit
 * carries what is on screen. The note it was copied from is left where it is.
 *
 * A note whose file was deleted outside Writ has no file to fall back to, and
 * the copy is the whole point of the offer, so the text the store kept for it
 * is read before disk is.
 */
export async function saveCopyOfNote(id: string): Promise<void> {
  const win = windowRegistry.getActive();
  if (!win) return;

  try {
    const live = win.editor.currentBufferId() === id ? win.editor.getActiveText(false) : null;
    const kept = win.editor.textOfRemoved(id);
    const content = live ? live.text : (kept ?? (await bufferRegistry.readContent(id)));
    const path = await bufferRegistry.saveCopy(id, content);
    await win.tabs.openFile(path);
  } catch {
    logFailure("a copy of a note could not be written");
    showToast(`Couldn't copy "${noteName(id)}"`, "error");
  }
}

/** Opens the platform's file manager with the note selected. */
export async function showNoteInFileManager(id: string): Promise<void> {
  try {
    await api.showNoteInFileManager(id);
  } catch {
    logFailure("a note could not be shown in the file manager");
    showToast("Could not open the file manager.", "error");
  }
}

/** [`showNoteInFileManager`] for a row the sidebar names by path. */
export async function showNotesFileInFileManager(path: string): Promise<void> {
  try {
    await api.showNotesFileInFileManager(path);
  } catch {
    logFailure("a note could not be shown in the file manager");
    showToast("Could not open the file manager.", "error");
  }
}
