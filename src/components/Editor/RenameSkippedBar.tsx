import { For, Show } from "solid-js";
import { renameLinksStore } from "../../stores/global/rename-links";
import { unchangedHeading } from "../../lib/rename-copy";
import "./RenameSkippedBar.css";

/**
 * The bar naming the notes a rename could not rewrite.
 *
 * A file that was not downloaded, one another program had just written and one
 * the filesystem would not take is left holding a link to a name no note
 * answers to any more. That is the person's to fix, so it is said in a bar
 * that stays until they have read it rather than in a toast that takes the
 * list away after four seconds.
 */
export default function RenameSkippedBar() {
  const notes = () => renameLinksStore.skippedNotes();
  return (
    <Show when={notes().length > 0}>
      <div class="rename-skipped-bar" role="status">
        <div class="rename-skipped-bar-text">
          <p class="rename-skipped-bar-heading">{unchangedHeading(notes().length)}</p>
          <ul class="rename-skipped-bar-list">
            <For each={notes()}>
              {(note) => (
                <li>
                  {note.name}: {note.reason}
                </li>
              )}
            </For>
          </ul>
        </div>
        <button
          type="button"
          class="rename-skipped-bar-action"
          onClick={() => renameLinksStore.clearSkipped()}
        >
          Dismiss
        </button>
      </div>
    </Show>
  );
}
