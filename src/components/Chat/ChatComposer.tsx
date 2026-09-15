import { For, Show, createSignal, onCleanup } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import MentionPopover from "./MentionPopover";
import ModelPicker from "./ModelPicker";
import { linkStore } from "../../stores/global/link";
import { chatStore } from "../../stores/global/chat";
import { byteLabel, sendChatMessage } from "../../commands/chat";
import type { NoteNameHit } from "../../stores/global/link";

/** How long typing settles before the note list is asked again. */
const MENTION_DEBOUNCE_MS = 120;
const MENTION_LIMIT = 8;

/** The `@` word the caret sits in, or null when it sits in none. */
export function mentionQuery(text: string, caret: number): { query: string; start: number } | null {
  const before = text.slice(0, caret);
  const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
  if (!match) return null;
  return { query: match[1], start: before.length - match[1].length - 1 };
}

/**
 * What the next message carries: the words, the notes, and the model that
 * answers.
 *
 * The chips are the whole of what leaves the machine (ADR-031 rule 2.5), and
 * every one of them got here by a person's action: the note in front when the
 * pane opened, or an `@` picked by hand.
 */
export default function ChatComposer() {
  let input: HTMLTextAreaElement | undefined;
  const [hits, setHits] = createSignal<NoteNameHit[]>([]);
  const [active, setActive] = createSignal(0);
  const [mention, setMention] = createSignal<{ query: string; start: number } | null>(null);
  let debounce: ReturnType<typeof setTimeout> | null = null;

  onCleanup(() => {
    if (debounce !== null) clearTimeout(debounce);
  });

  const busy = () => chatStore.status() === "thinking" || chatStore.status() === "streaming";
  const isOpen = () => mention() !== null;

  function closeMention() {
    if (debounce !== null) {
      clearTimeout(debounce);
      debounce = null;
    }
    setMention(null);
    setHits([]);
    setActive(0);
  }

  function onInput(event: InputEvent & { currentTarget: HTMLTextAreaElement }) {
    const el = event.currentTarget;
    chatStore.setDraft(el.value);
    const found = mentionQuery(el.value, el.selectionStart);
    if (!found) {
      closeMention();
      return;
    }
    setMention(found);
    setActive(0);
    if (debounce !== null) clearTimeout(debounce);
    debounce = setTimeout(() => {
      debounce = null;
      void linkStore.noteNameCandidates(found.query, MENTION_LIMIT).then((found) => {
        if (isOpen()) setHits(found);
      });
    }, MENTION_DEBOUNCE_MS);
  }

  /** Takes the `@query` back out of the draft and attaches what it named. */
  function pick(hit: NoteNameHit) {
    const found = mention();
    if (!found || !input) return;
    const text = chatStore.draft();
    const caret = input.selectionStart;
    const next = text.slice(0, found.start) + text.slice(caret);
    chatStore.setDraft(next);
    input.value = next;
    input.setSelectionRange(found.start, found.start);
    closeMention();
    void chatStore.attachByPath(hit.path);
    input.focus();
  }

  function onKeyDown(event: KeyboardEvent) {
    if (isOpen()) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setActive((at) => (hits().length === 0 ? 0 : (at + 1) % hits().length));
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setActive((at) => (hits().length === 0 ? 0 : (at - 1 + hits().length) % hits().length));
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        closeMention();
        return;
      }
      if (event.key === "Enter" && !event.shiftKey) {
        const hit = hits()[active()];
        if (hit) {
          event.preventDefault();
          pick(hit);
          return;
        }
      }
    }
    if (event.key === "Escape" && chatStore.editing() !== null) {
      event.preventDefault();
      chatStore.cancelEdit();
      return;
    }
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void sendChatMessage();
  }

  return (
    <div class="chat-composer">
      <Show when={chatStore.editing() !== null}>
        <p class="chat-composer-editing">
          Sending replaces this message and everything after it.
        </p>
      </Show>

      <Show when={chatStore.attachments().length > 0}>
        <ul class="chat-chips" aria-label="Notes it can read">
          <For each={chatStore.attachments()}>
            {(note) => (
              <li class="chat-chip">
                <Icon name="file-text" size={12} />
                <span class="chat-chip-name">{note.name}</span>
                <span class="chat-chip-size">{byteLabel(note.bytes)}</span>
                <button
                  type="button"
                  class="chat-chip-remove"
                  aria-label={`Remove ${note.name}`}
                  onClick={() => chatStore.detach(note.path)}
                >
                  <Icon name="x" size={10} />
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>

      <div class="chat-composer-field">
        <Show when={isOpen()}>
          <MentionPopover hits={hits()} active={active()} onPick={pick} />
        </Show>
        <textarea
          class="chat-composer-input"
          rows={3}
          spellcheck={false}
          placeholder="Ask about the attached notes. @ attaches another."
          aria-label="Message"
          ref={input}
          value={chatStore.draft()}
          onInput={onInput}
          onKeyDown={onKeyDown}
          onBlur={closeMention}
        />
      </div>

      <div class="chat-composer-actions">
        <ModelPicker live={chatStore.liveModels()} />
        <Show when={chatStore.editing() !== null}>
          <Button onClick={() => chatStore.cancelEdit()}>Cancel</Button>
        </Show>
        <Show
          when={busy()}
          fallback={
            <Button
              variant="primary"
              disabled={chatStore.draft().trim().length === 0}
              onClick={() => void sendChatMessage()}
            >
              Send
            </Button>
          }
        >
          <Button onClick={() => chatStore.stop()}>Stop</Button>
        </Show>
      </div>
    </div>
  );
}
