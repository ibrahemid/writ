import { For, Show } from "solid-js";
import { renameLinksStore } from "../../stores/global/rename-links";
import { unchangedHeading } from "../../lib/rename-copy";
import Button from "../Button/Button";
import "./EditorBar.css";

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
      <div class="editor-bar rename-skipped-bar" role="status">
        <div class="editor-bar-text">
          <p class="editor-bar-heading">{unchangedHeading(notes().length)}</p>
          <ul class="editor-bar-list">
            <For each={notes()}>
              {(note) => (
                <li>
                  {note.name}: {note.reason}
                </li>
              )}
            </For>
          </ul>
        </div>
        <Button onClick={() => renameLinksStore.clearSkipped()}>Dismiss</Button>
      </div>
    </Show>
  );
}
