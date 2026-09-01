import { createSignal, Show } from "solid-js";
import { saveStatusStore } from "../../stores/global/save-status";
import { saveCopyOfNote } from "../../lib/note-actions";
import { useWindow } from "../WindowProvider/WindowProvider";
import "./SaveFailureBar.css";

// The code for a file whose bytes are not on this machine. Asking the file
// before writing again is what stops a press landing on a placeholder.
const ERR_FILE_NOT_DOWNLOADED = "ERR_FILE_NOT_DOWNLOADED";

/**
 * The bar a note carries while its text is not on disk.
 *
 * One bar, for the note in front, staying until a write lands. A save failure
 * used to be a toast that took the reason away after four seconds, which is
 * the one thing a failure must not do: the text is still only in the editor,
 * and the person has to be able to read what happened and act on it whenever
 * they look up.
 */
export default function SaveFailureBar(props: { noteId: string | null }) {
  const win = useWindow();
  const [retrying, setRetrying] = createSignal(false);
  const [stillWaiting, setStillWaiting] = createSignal(false);

  const status = () => {
    const id = props.noteId;
    return id === null ? null : saveStatusStore.forNote(id);
  };
  const failed = () => {
    const current = status();
    return current !== null && current.state === "failed" ? current : null;
  };

  async function tryAgain(id: string, code: string | null) {
    setRetrying(true);
    setStillWaiting(false);
    try {
      if (code === ERR_FILE_NOT_DOWNLOADED && (await win.editor.readDiskState(id)) === null) {
        setStillWaiting(true);
        return;
      }
      await win.editor.retrySave(id);
    } finally {
      setRetrying(false);
    }
  }

  return (
    <Show when={failed()}>
      {(current) => (
        <div class="save-failure-bar" role="alert">
          <p class="save-failure-bar-text">
            Couldn't save {current().fileName}: {current().reason?.message}
            <Show when={stillWaiting()}>
              {" "}
              <span class="save-failure-bar-note">Still downloading.</span>
            </Show>
          </p>
          <div class="save-failure-bar-actions">
            <Show when={current().reason?.retryable !== false}>
              <button
                type="button"
                class="save-failure-bar-action"
                disabled={retrying()}
                onClick={() => void tryAgain(props.noteId!, current().reason?.code ?? null)}
              >
                Try again
              </button>
            </Show>
            <button
              type="button"
              class="save-failure-bar-action"
              onClick={() => void saveCopyOfNote(props.noteId!)}
            >
              Save a copy…
            </button>
          </div>
        </div>
      )}
    </Show>
  );
}
