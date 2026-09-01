import { Show } from "solid-js";
import { saveStatusStore } from "../../stores/global/save-status";
import "./SaveMarker.css";

/**
 * The mark a tab carries while its text is not on disk.
 *
 * The two states are different shapes, a ring and a triangle, rather than one
 * shape in two colours: colour alone is not a distinction everyone can make
 * (A1). The label is what a screen reader reads out as part of the tab's name.
 */
export default function SaveMarker(props: { noteId: string }) {
  const state = () => saveStatusStore.stateOf(props.noteId);

  return (
    <Show when={state() === "dirty" || state() === "failed"}>
      <span
        class="save-marker"
        classList={{ "save-marker--failed": state() === "failed" }}
        role="img"
        aria-label={state() === "failed" ? "not saved" : "unsaved changes"}
      />
    </Show>
  );
}
