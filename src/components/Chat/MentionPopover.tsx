import { For, Show } from "solid-js";
import Icon from "../Icon/Icon";
import type { NoteNameHit } from "../../stores/global/link";

/** The notes an `@` can reach, as the composer offers them. */
export default function MentionPopover(props: {
  hits: NoteNameHit[];
  active: number;
  onPick: (hit: NoteNameHit) => void;
}) {
  return (
    <div class="chat-mention" role="listbox" aria-label="Notes">
      <Show when={props.hits.length > 0} fallback={<p class="chat-empty">No note by that name.</p>}>
        <For each={props.hits}>
          {(hit, index) => (
            <button
              type="button"
              role="option"
              class="chat-mention-row"
              classList={{ "is-active": index() === props.active }}
              aria-selected={index() === props.active}
              onMouseDown={(event) => {
                // The composer keeps focus, so the caret stays where the pick
                // has to land.
                event.preventDefault();
                props.onPick(hit);
              }}
            >
              <Icon name="file-text" size={14} />
              <span class="chat-mention-name">{hit.name}</span>
            </button>
          )}
        </For>
      </Show>
    </div>
  );
}
