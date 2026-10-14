import { bufferRegistry } from "../stores/global/buffer-registry";
import { notesStore } from "../stores/global/notes";
import { saveStatusStore } from "../stores/global/save-status";
import { windowRegistry } from "../stores/global/window-registry";
import { requestChoice, requestConfirm } from "../stores/global/dialogs";
import { renameLinksStore } from "../stores/global/rename-links";
import { reportOpenOutcome } from "../stores/global/open-report";
import { linkCountQuestion } from "./rename-copy";
import { showToast } from "../stores/global/notifications";
import { logFailure } from "./log";
import { noteName } from "./note-name";
import { basename } from "./path";
import { formatSaveError } from "./save-error";
import { detectPlatform } from "./platform";
import { noteFiles } from "../stores/global/note-files";
import type { ChangeChoice, ResolveOutcome } from "../types/buffer";
import { userErrorText } from "./error-text";

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
      message: "Links in other files are rewritten to the new name.",
      confirmLabel: "Update links",
      cancelLabel: "Rename only",
    });
    if (answer === "dismissed") return;
    updateLinks = answer === "confirm";
  }
  const outcome = await renameLinksStore.renameWithLinks(id, title, updateLinks);
  const updated = outcome?.updated_paths.length ?? 0;
  if (outcome === null || updated === 0) return;
  const token = outcome.undo_token;
  showToast({
    text: updated === 1 ? "Updated links in 1 file" : `Updated links in ${updated} files`,
    type: "success",
    action:
      token === null
        ? undefined
        : { label: "Undo", run: () => undoRenameFromToast(token) },
  });
}

async function undoRenameFromToast(token: string): Promise<void> {
  try {
    await renameLinksStore.undoRename(token);
  } catch (error) {
    logFailure("a rename could not be undone");
    showToast(`Could not undo the rename: ${formatSaveError(error)}`, "error");
  }
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
 * Asks, then moves the note to the Trash and closes its tab. The toast that
 * follows carries an Undo that writes the file back where it was and opens it.
 */
export async function confirmAndDeleteNote(id: string): Promise<void> {
  const name = noteName(id);
  if (!noteIsDeletable(id)) {
    showToast("Only files in your folder can be moved to the Trash from here.", "error");
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
  let undo: string | null;
  try {
    undo = win ? await win.tabs.deleteNote(id) : await bufferRegistry.deleteNote(id);
  } catch {
    logFailure("a note could not be moved to the trash");
    showToast(`Could not move "${name}" to the Trash.`, "error");
    return;
  }
  showToast({
    text: `Moved "${name}" to the Trash`,
    type: "success",
    action: undo === null ? undefined : { label: "Undo", run: () => putBackFromTrash(undo, name) },
  });
}

/** Puts a trashed note back at its path and opens it again. */
async function putBackFromTrash(token: string, name: string): Promise<void> {
  let path: string;
  try {
    path = await bufferRegistry.undoDeleteNote(token);
  } catch (error) {
    logFailure("a trashed note could not be put back");
    const reason = userErrorText(error);
    showToast(`Could not put "${name}" back.${reason ? ` ${reason}` : ""}`, "error");
    return;
  }
  const win = windowRegistry.getActive();
  if (!win) return;
  reportOpenOutcome(await win.tabs.openPath(path));
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
    showToast(`Could not copy "${noteName(id)}"`, "error");
  }
}

/**
 * Saves a note whose first file could not be made to a place the person picks
 * in the save panel, which becomes the note's file.
 *
 * Cancelling the panel changes nothing. A write that does not land reports to
 * the save failure bar like any other, and the note stays held without a file.
 */
export async function saveNoteAs(id: string): Promise<void> {
  const win = windowRegistry.getActive();
  if (!win) return;

  let path: string | null;
  try {
    path = await noteFiles.pickSavePath(id);
  } catch {
    logFailure("the save panel for a note with no file could not be shown");
    showToast(`Could not save "${noteName(id)}"`, "error");
    return;
  }
  if (path === null) return;

  const result = await win.editor.saveRemovedNoteTo(id, path);
  if (result.ok) await bufferRegistry.refreshBuffer(id).catch(() => undefined);
}

/**
 * Carries out what the person chose about a file that changed outside Writ.
 *
 * The text sent is the one on screen, because that is the only place the
 * unsaved version of the note exists. What comes back is what the tab must do
 * next: take the file's text, or keep its own and stop reading dirty against
 * a file it has just been written to.
 *
 * `Keep both` opens the copy as an ordinary note in a second tab. There is no
 * diff view and no third state: two tabs, two files, and the person decides.
 * The other two answers name the copy in a toast that opens it.
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
  const inView = win.editor.currentBufferId() === id;
  const live = inView ? win.editor.getActiveText(false) : null;
  const read = inView ? win.editor.liveDoc() : null;
  if (!live || !read) {
    logFailure("a change outside Writ was answered for a note the editor was not holding");
    return;
  }
  const content = live.text;

  let outcome: ResolveOutcome;
  try {
    outcome = await noteFiles.resolveExternalChange(id, choice, content);
  } catch (error) {
    logFailure("a change outside Writ could not be resolved");
    showToast(`Could not update "${noteName(id)}": ${formatSaveError(error)}`, "error");
    return;
  }

  // Anything typed between the read above and here was held rather than
  // queued, because the note was still waiting to be answered about. Read
  // before the answer ends the hold, because ending it empties the slot the
  // typing is in, and read through the store rather than off the view: the
  // note is not always the one on screen by the time the answer lands, and a
  // guard on that would drop the typing of every tab switched away from
  // mid-answer.
  //
  // The write carried the note's document only when the store can say what
  // the note holds now and it is that text. With nothing to say it, typing
  // during the answer may already have left the hold in a write of its own (a
  // file found at another path ends the hold), so the note reads dirty.
  const typedSince = win.editor.liveTextOf(id);

  // A save that failed against this same change left a bar of its own, about a
  // write the answer has just made irrelevant. It goes with the answer, or it
  // sits on the tab afterwards saying the note could not be written when it
  // has just been. A write already on the wire when the answer landed is about
  // the same superseded file, so it goes too even though its refusal arrives
  // later; a write issued after the answer still shows.
  saveStatusStore.forgetWritesSoFar(id);
  win.editor.recordFileEvent(id, "settled");
  if (outcome.content !== null) {
    win.editor.applyExternalContent(id, outcome.content);
  } else {
    win.editor.noteSaved(id, outcome.disk_hash, false, {
      source: read,
      current: typedSince?.current === true && typedSince.content === content,
    });
    // The write that has landed carried the text as it was read, so the typing
    // above is in the document and nowhere else. It goes on the queue the way
    // a keystroke would put it there, and reaches the file under the same rate
    // cap every other write obeys.
    //
    // The tab keeps the delta on the way out too, because the queue is what
    // the close and quit paths hand to the unsaved-text journal.
    //
    // Only this branch has a delta to keep. `Use the file on disk` replaces
    // the document with what the file holds, on purpose, and there is nothing
    // to merge the typing onto.
    if (typedSince !== undefined && typedSince.content !== content) {
      win.editor.scheduleAutosave(id, typedSince.content, 0, typedSince.current);
    }
  }
  const copy = outcome.conflict_copy_path;
  if (copy === null) return;
  if (choice === "keep_both") {
    await openCopy(copy);
    return;
  }
  showToast({
    text: `The other version is saved as "${basename(copy)}"`,
    type: "success",
    action: { label: "Open", run: () => openCopy(copy) },
  });
}

async function openCopy(path: string): Promise<void> {
  const win = windowRegistry.getActive();
  if (!win) return;
  try {
    await win.tabs.openFile(path);
  } catch {
    logFailure("the copy beside a note could not be opened");
    showToast("Could not open the copy.", "error");
  }
}

/** Opens the platform's file manager with the note selected. */
export async function showNoteInFileManager(id: string): Promise<void> {
  try {
    await noteFiles.showInFileManager(id);
  } catch {
    logFailure("a note could not be shown in the file manager");
    showToast("Could not open the file manager.", "error");
  }
}

/** [`showNoteInFileManager`] for a row the sidebar names by path. */
export async function showNotesFileInFileManager(path: string): Promise<void> {
  try {
    await noteFiles.showPathInFileManager(path);
  } catch {
    logFailure("a note could not be shown in the file manager");
    showToast("Could not open the file manager.", "error");
  }
}
