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
  // Held as the note each answer is about rather than as a flag, because one
  // bar serves every tab: a flag set for the note in front is still set when
  // the next tab's failure renders through the same component, and that tab
  // would be told about a download that is not its own.
  const [retryingNote, setRetryingNote] = createSignal<string | null>(null);
  const [waitingNote, setWaitingNote] = createSignal<string | null>(null);
  const retrying = () =>
    retryingNote() !== null && retryingNote() === props.noteId;
  const stillWaiting = () =>
    waitingNote() !== null && waitingNote() === props.noteId;

  const status = () => {
    const id = props.noteId;
    return id === null ? null : saveStatusStore.forNote(id);
  };
  const failed = () => {
    const current = status();
    return current !== null && current.state === "failed" ? current : null;
  };
  // A save on the wire when the watcher reported fails under the question the
  // bar above is asking, and it can fail for a reason worth pressing again.
  // The press would reach the same hold every other write path reaches and
  // change nothing on screen, so the button goes while the hold is on, the way
  // an unretryable reason takes it away. `Save a copy…` is not held and stays:
  // it is the one thing that gets the text out of a tab that cannot write.
  const heldForAnswer = () =>
    props.noteId !== null && win.editor.savesAreHeld(props.noteId);

  async function tryAgain(id: string, code: string | null) {
    setRetryingNote(id);
    setWaitingNote(null);
    try {
      if (
        code === ERR_FILE_NOT_DOWNLOADED &&
        (await win.editor.readDiskState(id)).state !== "described"
      ) {
        setWaitingNote(id);
        return;
      }
      await win.editor.retrySave(id);
    } finally {
      setRetryingNote((current) => (current === id ? null : current));
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
            <Show
              when={current().reason?.retryable !== false && !heldForAnswer()}
            >
              <button
                type="button"
                class="save-failure-bar-action"
                disabled={retrying()}
                onClick={() =>
                  void tryAgain(props.noteId!, current().reason?.code ?? null)
                }
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
