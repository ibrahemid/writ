import { bufferRegistry } from "../stores/global/buffer-registry";
import { notesStore } from "../stores/global/notes";
import { saveStatusStore } from "../stores/global/save-status";
import { windowRegistry } from "../stores/global/window-registry";
import { requestConfirm } from "../components/ConfirmDialog/ConfirmDialog";
import { showToast } from "../components/Notifications/Toast";
import { logFailure } from "./log";
import { noteName } from "./note-name";
import { formatSaveError } from "./save-error";
import { detectPlatform } from "./platform";
import * as api from "../services/tauri";
import type { ChangeChoice, ResolveOutcome } from "../types/buffer";

export { noteName };

// The note operations three surfaces share: the command palette, the tab
// context menu and the file tree. Keeping them here is what stops the same
// confirmation being worded three ways.


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
 */
export async function saveCopyOfNote(id: string): Promise<void> {
  const win = windowRegistry.getActive();
  if (!win) return;

  const live = win.editor.currentBufferId() === id ? win.editor.getActiveText(false) : null;
  const content = live ? live.text : await bufferRegistry.readContent(id);

  try {
    const path = await bufferRegistry.saveCopy(id, content);
    await win.tabs.openFile(path);
  } catch {
    logFailure("a copy of a note could not be written");
    showToast(`Couldn't copy "${noteName(id)}"`, "error");
  }
}

/**
 * Carries out what the person chose about a file that changed outside Writ.
 *
 * The text sent is the one on screen, because that is the only place the
 * unsaved version of the note exists. What comes back is what the tab must do
 * next: take the file's text, or keep its own and stop reading dirty against
 * a file it has just been written to.
 *
 * `Show both` opens the copy as an ordinary note in a second tab. There is no
 * diff view and no third state: two tabs, two files, and the person decides.
 *
 * Nothing happens unless the editor is holding the note being answered about.
 * A tab that is still loading has its text nowhere but its file, and the file
 * is the version that changed, so `Keep mine` on that half-second would send
 * the file's own text as mine and write the unsaved version nowhere. The bar
 * stays and the next press lands.
 */
export async function resolveNoteChange(
  id: string,
  choice: ChangeChoice,
): Promise<void> {
  const win = windowRegistry.getActive();
  if (!win) return;
  const live =
    win.editor.currentBufferId() === id ? win.editor.getActiveText(false) : null;
  if (!live) {
    logFailure("a change outside Writ was answered for a note the editor was not holding");
    return;
  }
  const content = live.text;

  let outcome: ResolveOutcome;
  try {
    outcome = await api.resolveExternalChange(id, choice, content);
  } catch (error) {
    logFailure("a change outside Writ could not be resolved");
    showToast(`Couldn't update "${noteName(id)}": ${formatSaveError(error)}`, "error");
    return;
  }

  // A save that failed against this same change left a bar of its own, about a
  // write the answer has just made irrelevant. It goes with the answer, or it
  // sits on the tab afterwards saying the note could not be written when it
  // has just been. Only a failure raised before the answer: one the answer
  // itself raises is recorded after this and still shows.
  saveStatusStore.forgetNote(id);
  win.editor.recordFileEvent(id, "settled");
  if (outcome.content !== null) {
    win.editor.applyExternalContent(id, outcome.content);
  } else {
    win.editor.noteSaved(id, outcome.disk_hash);
  }
  if (choice === "keep_both" && outcome.conflict_copy_path !== null) {
    try {
      await win.tabs.openFile(outcome.conflict_copy_path);
    } catch {
      logFailure("the copy beside a note could not be opened");
      showToast("Couldn't open the copy.", "error");
    }
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
