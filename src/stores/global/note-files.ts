import * as api from "../../services/tauri";
import type { ChangeChoice, ResolveOutcome } from "../../types/buffer";

/** The file side of a note's actions: the save panel, a change outside Writ, the file manager. */
export const noteFiles = {
  pickSavePath(id: string): Promise<string | null> {
    return api.pickNoteSavePath(id);
  },
  resolveExternalChange(
    id: string,
    choice: ChangeChoice,
    content: string,
  ): Promise<ResolveOutcome> {
    return api.resolveExternalChange(id, choice, content);
  },
  showInFileManager(id: string): Promise<void> {
    return api.showNoteInFileManager(id);
  },
  showPathInFileManager(path: string): Promise<void> {
    return api.showNotesFileInFileManager(path);
  },
};
