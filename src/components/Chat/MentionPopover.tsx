import { For, Show, createEffect } from "solid-js";
import Icon from "../Icon/Icon";
import type { NoteNameHit } from "../../stores/global/link";

/** The element the field points `aria-controls` at. */
export const MENTION_LIST_ID = "chat-mention-list";

/** The id of one row, which the field names while that row is the active one. */
export function mentionRowId(index: number): string {
  return `chat-mention-${index}`;
}

/** The folder a hit sits in, or nothing for a note at the root. Two notes of
 * one name in two folders are otherwise two identical rows. */
export function mentionFolder(path: string): string {
  const parts = path.split(/[\\/]/).filter((part) => part.length > 0);
  return parts.slice(0, -1).join("/");
}

/** The notes an `@` can reach, as the composer offers them. */
export default function MentionPopover(props: {
  hits: NoteNameHit[];
  active: number;
  onPick: (hit: NoteNameHit) => void;
}) {
  const rows: HTMLElement[] = [];

  // The list scrolls, so arrowing past its last visible row brings the active
  // one into view: a highlight nobody can see is not a choice being offered.
  createEffect(() => {
    const at = props.active;
    void props.hits.length;
    rows[at]?.scrollIntoView?.({ block: "nearest" });
  });

  return (
    <div class="chat-mention">
      <div class="chat-mention-list" id={MENTION_LIST_ID} role="listbox" aria-label="Notes">
        <For each={props.hits}>
          {(hit, index) => (
            <div
              id={mentionRowId(index())}
              role="option"
              tabindex={-1}
              class="chat-mention-row"
              classList={{ "is-active": index() === props.active }}
              aria-selected={index() === props.active}
              ref={(el) => {
                rows[index()] = el;
              }}
              onMouseDown={(event) => {
                // The composer keeps focus, so the caret stays where the pick
                // has to land.
                event.preventDefault();
                props.onPick(hit);
              }}
            >
              <Icon name="file-text" size={14} />
              <span class="chat-mention-name">{hit.name}</span>
              <Show when={mentionFolder(hit.path)}>
                {(folder) => <span class="chat-mention-folder">{folder()}</span>}
              </Show>
            </div>
          )}
        </For>
      </div>
      <Show when={props.hits.length === 0}>
        <p class="chat-empty">No note by that name.</p>
      </Show>
    </div>
  );
}
