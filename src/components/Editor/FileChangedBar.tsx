import { createEffect, Show } from "solid-js";
import { resolveNoteChange } from "../../lib/note-actions";
import { useWindow } from "../WindowProvider/WindowProvider";
import type { ChangeChoice } from "../../types/buffer";
import "./FileChangedBar.css";

/**
 * The bar a note carries when its file changed while the document held text
 * the file does not.
 *
 * It is a question, and it stays until it is answered: nothing is replaced,
 * nothing is written, and the queued save was dropped before it appeared. All
 * three answers keep both texts — the one that loses is written to its own
 * file first — so there is no wrong button here, only a slower way back.
 */
export default function FileChangedBar(props: { noteId: string | null }) {
  const win = useWindow();
  let firstAction: HTMLButtonElement | undefined;

  // Asked only while the editor is holding this note. A tab that is still
  // loading has its text nowhere but its file, and the file is the version
  // that changed, so there is no version of "mine" to keep yet. The question
  // appears the moment the document does.
  const changed = () => {
    const id = props.noteId;
    if (id === null || win.editor.currentBufferId() !== id) return null;
    return win.editor.isFileChangedOnDisk(id) ? id : null;
  };

  // The question is the reason the editor stopped, so it takes the focus that
  // was in the editor. Answering it hands the focus straight back, and
  // CodeMirror puts the cursor back where it was holding it. A save asked for
  // while the question is up brings the focus back here too, which is what
  // that keystroke means for this note.
  createEffect(() => {
    win.editor.pendingChangeAnswer();
    if (changed() !== null) firstAction?.focus();
  });

  async function answer(id: string, choice: ChangeChoice) {
    await resolveNoteChange(id, choice);
    win.editor.focusEditor();
  }

  return (
    <Show when={changed()}>
      {(id) => (
        <div
          class="file-changed-bar"
          role="alertdialog"
          aria-labelledby="file-changed-bar-text"
        >
          <p class="file-changed-bar-text" id="file-changed-bar-text">
            This file changed outside Writ.
          </p>
          <div class="file-changed-bar-actions">
            <button
              type="button"
              class="file-changed-bar-action"
              ref={firstAction}
              onClick={() => void answer(id(), "keep_mine")}
            >
              Keep mine
            </button>
            <button
              type="button"
              class="file-changed-bar-action"
              onClick={() => void answer(id(), "use_disk")}
            >
              Use the file on disk
            </button>
            <button
              type="button"
              class="file-changed-bar-action"
              onClick={() => void answer(id(), "keep_both")}
            >
              Show both
            </button>
          </div>
        </div>
      )}
    </Show>
  );
}
