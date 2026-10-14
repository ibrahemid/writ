import { onCleanup, onMount, Show } from "solid-js";
import { captureNoteStore } from "../../stores/capture/capture-note";
import "./CaptureNote.css";

/**
 * The quick capture note (ADR-055): a field whose text Escape adds to the end
 * of today's file. It lives in its own window, so it reaches nothing of the
 * main window's.
 */
export default function CaptureNote(props: { note?: typeof captureNoteStore }) {
  const note = props.note ?? captureNoteStore;
  let field: HTMLTextAreaElement | undefined;

  onMount(() => {
    // The window keeps its page while hidden, so each show is a focus event,
    // not a mount.
    const onShown = () => {
      field?.focus();
      void note.refreshTarget();
    };
    const onLeft = () => void note.dismiss();
    window.addEventListener("focus", onShown);
    window.addEventListener("blur", onLeft);
    onShown();
    onCleanup(() => {
      window.removeEventListener("focus", onShown);
      window.removeEventListener("blur", onLeft);
    });
  });

  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== "Escape" || event.isComposing) return;
    event.preventDefault();
    void note.submit();
  }

  const destination = () => note.target() || "today's file";

  return (
    <div class="capture-note" aria-busy={note.busy()}>
      <textarea
        ref={field}
        class="capture-note-field"
        data-writ-focus-silent
        aria-label="Quick capture"
        value={note.text()}
        onInput={(event) => note.setText(event.currentTarget.value)}
        onKeyDown={onKeyDown}
      />
      <div class="capture-note-footer">
        <Show
          when={note.problem()}
          fallback={
            <span class="capture-note-hint">
              <kbd class="capture-note-key">Esc</kbd> adds it to {destination()}
            </span>
          }
        >
          {(problem) => (
            <span class="capture-note-problem" role="alert">
              {problem()}
            </span>
          )}
        </Show>
      </div>
    </div>
  );
}
