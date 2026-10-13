import { Show } from "solid-js";
import { noteName, saveCopyOfNote, saveNoteAs } from "../../lib/note-actions";
import { useWindow } from "../WindowProvider/WindowProvider";
import Button from "../Button/Button";
import "./EditorBar.css";

/**
 * The bar a note carries once its file is gone from disk.
 *
 * The text stays in the editor and no keystroke writes it back, because that
 * would recreate a file the person deleted somewhere else (spec W4). What
 * happens to the text is the person's call, and the three ways out are the
 * three the bar offers: put the file back where it was, write the text to a
 * new file, or let the tab go.
 *
 * A note the launch could not give its first file is held the same way, but
 * it was never deleted: the bar names the file it was to be and the folder
 * that would not take it, and offers a place the person picks instead.
 */
export default function RemovedOnDiskBar(props: { noteId: string | null }) {
  const win = useWindow();
  // A note whose read failed shows the failure in place of a document, so
  // there is no text here and nothing for "Put the file back" to write.
  const removed = () => {
    const id = props.noteId;
    if (id === null || !win.editor.isRemovedOnDisk(id)) return null;
    return win.editor.readFailure()?.bufferId === id ? null : id;
  };
  const name = () => {
    const id = removed();
    return id === null ? "" : noteName(id);
  };
  const unmade = () => {
    const id = removed();
    return id === null ? null : win.editor.unmadeFileOf(id);
  };

  return (
    <Show when={removed()}>
      {(id) => (
        <Show
          when={unmade()}
          fallback={
            <div class="editor-bar removed-on-disk-bar" role="alert">
              <p class="editor-bar-text">{name()} was deleted. Your text is still here.</p>
              <div class="editor-bar-actions">
                <Button onClick={() => void win.editor.restoreRemovedFile(id())}>
                  Put the file back
                </Button>
                <Button onClick={() => void saveCopyOfNote(id())}>Save a copy…</Button>
                <Button onClick={() => void win.tabs.closeTab(id())}>Close</Button>
              </div>
            </div>
          }
        >
          {(file) => (
            <div class="editor-bar removed-on-disk-bar" role="alert">
              <p class="editor-bar-text">
                Couldn't create {file().name} in {file().folder}.
              </p>
              <div class="editor-bar-actions">
                <Button onClick={() => void saveNoteAs(id())}>Save As…</Button>
                <Button onClick={() => void win.tabs.closeTab(id())}>Close</Button>
              </div>
            </div>
          )}
        </Show>
      )}
    </Show>
  );
}
