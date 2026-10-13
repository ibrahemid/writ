import type { RecoveredBuffer, UnmadeFile } from "../../services/tauri";

/** The editor action a recovered note reaches, which is all it is given. */
export interface RecoveredNoteActions {
  markRemovedOnDisk(id: string, text?: string, unmade?: UnmadeFile): void;
}

/**
 * Hands the notes the launch recovered to the window's editor, and answers the
 * ones it restored, which the restore toast counts.
 *
 * Only notes whose text never reached their file are restored, so the count is
 * how many of those there were, not how many tabs were open. The launch lists
 * each note once, however many of its texts it wrote.
 *
 * A note whose file was deleted is not among them: the launch wrote nothing for
 * it (ADR-033 decision 15), so its text is seeded into the editor instead,
 * before any tab loads, and its tab comes up carrying the bar and the three
 * ways out. So is a note whose first file the launch could not make, under a
 * bar that names that file (ADR-044 §4). Counting either as restored would say
 * a file is back that is not.
 *
 * It lives here rather than inside `App`'s `onMount` so what reaches the editor
 * can be tested.
 */
export function applyRecoveredNotes(
  editor: RecoveredNoteActions,
  recovered: readonly RecoveredBuffer[],
): RecoveredBuffer[] {
  for (const note of recovered) {
    if (note.removed_on_disk) {
      editor.markRemovedOnDisk(note.id, note.content, note.unmade_file ?? undefined);
    }
  }
  return recovered.filter((note) => !note.removed_on_disk);
}
