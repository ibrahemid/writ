import { Show } from "solid-js";
import { noteName, saveCopyOfNote } from "../../lib/note-actions";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./RemovedOnDiskBar.css";

/**
 * The bar a note carries once its file is gone from disk.
 *
 * The text stays in the editor and no keystroke writes it back, because that
 * would recreate a file the person deleted somewhere else (spec W4). What
 * happens to the text is the person's call, and the three ways out are the
 * three the bar offers: put the file back where it was, write the text to a
 * new file, or let the tab go.
 */
export default function RemovedOnDiskBar(props: { noteId: string | null }) {
  const win = useWindow();
  const removed = () => {
    const id = props.noteId;
    return id !== null && win.editor.isRemovedOnDisk(id) ? id : null;
  };
  const name = () => {
    const id = removed();
    return id === null ? "" : noteName(id);
  };

  return (
    <Show when={removed()}>
      {(id) => (
        <div class="removed-on-disk-bar" role="alert">
          <p class="removed-on-disk-bar-text">
            {name()} was deleted. Your text is still here.
          </p>
          <div class="removed-on-disk-bar-actions">
            <button
              type="button"
              class="removed-on-disk-bar-action"
              onClick={() => void win.editor.restoreRemovedFile(id())}
            >
              Put the file back
            </button>
            <button
              type="button"
              class="removed-on-disk-bar-action"
              onClick={() => void saveCopyOfNote(id())}
            >
              Save a copy…
            </button>
            <button
              type="button"
              class="removed-on-disk-bar-action"
              onClick={() => void win.tabs.closeTab(id())}
            >
              Close
            </button>
          </div>
        </div>
      )}
    </Show>
  );
}
