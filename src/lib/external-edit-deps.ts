import type {
  ExternalEditBuffer,
  ExternalEditDeps,
} from "../services/external-edit";
import type { NoteFileEvent } from "../stores/window/editor-store";

/** An open note, as the row naming it is matched against the event. */
interface OpenBuffer extends ExternalEditBuffer {
  filename: string;
}

/** The editor store, as the response to a change outside Writ reaches it. */
interface ExternalEditEditor {
  isDirty: (id: string) => boolean;
  requestExternalReload: (id: string) => void;
  recordFileEvent: (id: string, event: NoteFileEvent) => void;
}

/** What the window and the global stores contribute to the response. */
export interface ExternalEditWiring {
  editor: ExternalEditEditor;
  openBuffers: () => readonly OpenBuffer[];
  /** Repoints the frontend's row for a note at the path Rust already moved it to. */
  refreshBuffer: (id: string) => Promise<unknown>;
  /** Drops the bar a save that could not land left behind. */
  forgetSaveStatus: (id: string) => void;
  cancelAutosave: (id: string) => void;
}

/**
 * Builds what `handleExternalEdit` acts through.
 *
 * A factory rather than an object literal at the subscription so the wiring
 * the app runs is the wiring the tests drive. Written inline it can only be
 * tested by copying it, and a copy goes on passing while the app keeps the
 * version that was copied from.
 *
 * Each response is one line about the file plus the tidying that goes with it.
 * The state the two bars read moves through `recordFileEvent`, which holds the
 * whole table: a note has one file, so it has one state, and only one bar can
 * be on screen to be answered.
 */
export function createExternalEditDeps(
  wiring: ExternalEditWiring,
): ExternalEditDeps {
  return {
    findBuffer: (key: string) =>
      wiring.openBuffers().find((b) => b.filename === key || b.id === key),
    // Whether the document differs from its file, not whether a save is
    // queued: a note whose autosave already landed still has unsaved work the
    // moment the next keystroke lands, and a note whose save was refused has
    // an empty queue and everything to lose.
    hasUnsaved: (id: string) => wiring.editor.isDirty(id),
    reload: (id: string) => wiring.editor.requestExternalReload(id),
    cancelAutosave: (id: string) => wiring.cancelAutosave(id),
    // A move changes no bytes, so nothing is read and nothing is asked: the
    // row already names the new path, and this is the tab catching up to it.
    followMove: (id: string) => {
      void wiring.refreshBuffer(id).catch(() => {});
      wiring.editor.recordFileEvent(id, "moved");
    },
    // The failure of a save that raced the deletion is about a file that is
    // no longer there, and its bar would sit under the one replacing it.
    markRemoved: (id: string) => {
      wiring.forgetSaveStatus(id);
      wiring.editor.recordFileEvent(id, "removed");
    },
    // The bar asks; nothing here decides. A save that failed against the same
    // change would otherwise leave its own bar under this one, saying two
    // things about one file.
    markChanged: (id: string) => {
      wiring.forgetSaveStatus(id);
      wiring.editor.recordFileEvent(id, "modified");
    },
  };
}
