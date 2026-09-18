import { For, Show, createEffect } from "solid-js";
import Icon from "../Icon/Icon";
import { noteCount } from "../../commands/chat";
import type { NoteFolderHit, NoteNameHit } from "../../stores/global/link";

/** One row the list offers: a note, or a folder with the notes under it. */
export type MentionRow =
  | { kind: "note"; hit: NoteNameHit }
  | { kind: "folder"; hit: NoteFolderHit };

/** The element the field points `aria-controls` at. */
export const MENTION_LIST_ID = "chat-mention-list";

/** The id of one row, which the field names while that row is the active one. */
export function mentionRowId(index: number): string {
  return `chat-mention-${index}`;
}

/** The notes and folders an `@` can reach, as the composer offers them. */
export default function MentionPopover(props: {
  rows: MentionRow[];
  active: number;
  onPick: (row: MentionRow) => void;
}) {
  const rows: HTMLElement[] = [];

  // The list scrolls, so arrowing past its last visible row brings the active
  // one into view: a highlight nobody can see is not a choice being offered.
  createEffect(() => {
    const at = props.active;
    void props.rows.length;
    rows[at]?.scrollIntoView?.({ block: "nearest" });
  });

  return (
    <div class="chat-mention">
      <div class="chat-mention-list" id={MENTION_LIST_ID} role="listbox" aria-label="Notes">
        <For each={props.rows}>
          {(row, index) => (
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
                props.onPick(row);
              }}
            >
              <Show
                when={row.kind === "folder" ? row.hit : null}
                fallback={
                  <>
                    <Icon name="file-text" size={14} />
                    <span class="chat-mention-name">
                      {row.kind === "note" ? row.hit.name : ""}
                    </span>
                    <Show when={row.kind === "note" && row.hit.folder}>
                      {(folder) => <span class="chat-mention-folder">{folder()}</span>}
                    </Show>
                  </>
                }
              >
                {(folder) => (
                  <>
                    <Icon name="folder" size={14} />
                    <span class="chat-mention-name">{`${folder().folder}/`}</span>
                    <span class="chat-mention-folder">{noteCount(folder().notes)}</span>
                  </>
                )}
              </Show>
            </div>
          )}
        </For>
      </div>
      <Show when={props.rows.length === 0}>
        <p class="chat-empty">No note by that name.</p>
      </Show>
    </div>
  );
}
