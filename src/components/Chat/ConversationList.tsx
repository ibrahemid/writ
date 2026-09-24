import { For, Show, createSignal } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import { requestConfirm } from "../ConfirmDialog/ConfirmDialog";
import { chatStore, type ChatConversationSummary } from "../../stores/global/chat";

/**
 * When a chat was last written to: the time today, the day and time yesterday,
 * the date and time before that, in the reader's own locale.
 */
export function chatTimeLabel(at: string, now: Date = new Date()): string {
  const stamp = new Date(at);
  if (Number.isNaN(stamp.getTime())) return "";
  const time = stamp.toLocaleTimeString(undefined, { hour: "2-digit", minute: "2-digit" });
  if (stamp.toDateString() === now.toDateString()) return time;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  if (stamp.toDateString() === yesterday.toDateString()) return `Yesterday ${time}`;
  return stamp.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

/** Every chat on disk, newest first. */
export default function ConversationList(props: { onPick: () => void }) {
  const [renaming, setRenaming] = createSignal<string | null>(null);

  async function remove(row: ChatConversationSummary) {
    const confirmed = await requestConfirm({
      title: `Delete ${row.title}?`,
      message: "The file goes with it.",
      confirmLabel: "Delete",
      danger: true,
      defaultAction: "cancel",
    });
    if (!confirmed) return;
    await chatStore.remove(row.id);
  }

  // Enter and Escape both close the field, and the blur of a field that goes
  // away lands here too; only a field still open for this row commits.
  async function commitRename(row: ChatConversationSummary, title: string) {
    if (renaming() !== row.id) return;
    setRenaming(null);
    const next = title.trim();
    if (!next || next === row.title) return;
    await chatStore.rename(row.id, next);
  }

  return (
    <section class="chat-chats" aria-labelledby="chat-chats-title">
      <h3 class="chat-section-title" id="chat-chats-title">
        Chats
      </h3>

      <Show
        when={chatStore.conversations().length > 0}
        fallback={<p class="chat-empty">No chats yet.</p>}
      >
        <ul class="chat-chats-list">
          <For each={chatStore.conversations()}>
            {(row) => (
              <li class="chat-chats-item">
                <Show
                  when={renaming() === row.id}
                  fallback={
                    <div
                      class="chat-chats-row"
                      classList={{ "is-current": chatStore.current()?.id === row.id }}
                    >
                      <button
                        type="button"
                        class="chat-chats-open"
                        aria-current={chatStore.current()?.id === row.id ? "true" : undefined}
                        onDblClick={() => setRenaming(row.id)}
                        onClick={() => {
                          void chatStore.open(row.id);
                          props.onPick();
                        }}
                      >
                        <Icon name="chat-text" size={14} />
                        <span class="chat-chats-title">{row.title}</span>
                        <span class="chat-chats-time">{chatTimeLabel(row.updated_at)}</span>
                      </button>
                      <Show when={chatStore.isLive(row.id)}>
                        <span class="chat-chats-live" aria-hidden="true" />
                        <Button
                          variant="ghost"
                          icon="square"
                          iconSize={10}
                          aria-label={`Stop ${row.title}`}
                          onClick={() => chatStore.stop(row.id)}
                        />
                      </Show>
                      <Button
                        variant="ghost"
                        icon="pencil-simple"
                        iconSize={12}
                        aria-label={`Rename ${row.title}`}
                        onClick={() => setRenaming(row.id)}
                      />
                      <Button
                        variant="ghost"
                        icon="trash"
                        iconSize={12}
                        aria-label={`Delete ${row.title}`}
                        onClick={() => void remove(row)}
                      />
                    </div>
                  }
                >
                  <input
                    class="chat-chats-rename"
                    value={row.title}
                    aria-label={`Rename ${row.title}`}
                    ref={(el) => queueMicrotask(() => el.select())}
                    onBlur={(event) => void commitRename(row, event.currentTarget.value)}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") {
                        event.preventDefault();
                        void commitRename(row, event.currentTarget.value);
                      } else if (event.key === "Escape") {
                        event.preventDefault();
                        setRenaming(null);
                      }
                    }}
                  />
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}
