import { For, Show, createEffect, createMemo, createSignal, onCleanup } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import MentionPopover, { MENTION_LIST_ID, mentionRowId } from "./MentionPopover";
import ChatConnectionControl from "./ChatConnectionControl";
import { linkStore } from "../../stores/global/link";
import { chatStore, chipLabel, noteName, type Attachment } from "../../stores/global/chat";
import { byteLabel, sendChatMessage } from "../../commands/chat";
import type { NoteNameHit } from "../../stores/global/link";

/** How long typing settles before the note list is asked again. */
const MENTION_DEBOUNCE_MS = 120;
const MENTION_LIMIT = 8;

/** Names the line that says why the note in front cannot be attached. */
const ADD_REASON_ID = "chat-add-open-note-reason";

/** Whether the note in front can be attached, and why it cannot. */
export type OpenNoteState = "ready" | "unsaved" | "none";

/** The `@` word the caret sits in, or null when it sits in none. */
export function mentionQuery(text: string, caret: number): { query: string; start: number } | null {
  const before = text.slice(0, caret);
  const match = /(?:^|\s)@([^\s@]*)$/.exec(before);
  if (!match) return null;
  return { query: match[1], start: before.length - match[1].length - 1 };
}

/** What a chip says when the pointer rests on it: the whole key, and for a
 * note with unsaved text which of the two versions travels. */
export function chipTitle(note: Attachment): string {
  const key = note.key ?? note.path;
  if (note.state === "unreadable") {
    return note.reason ? `${key}: ${note.reason}` : `${key} could not be read.`;
  }
  return note.dirty ? `Sends the saved version of ${key}` : key;
}

/**
 * What the next message carries: the words, the notes, and the model that
 * answers.
 *
 * The chips are the whole of what leaves the machine (ADR-031 rule 2.5), and
 * every one of them got here by a person's action: the note in front when the
 * pane opened, one added from here, or an `@` picked by hand.
 */
export default function ChatComposer(props: {
  /** Whether the note in front can be attached, which the pane resolves. */
  openNote: () => OpenNoteState;
  /** Closes the column, which is what Escape does with nothing else pending. */
  onClose: () => void;
}) {
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

  /** The chips a send cannot carry, which is what holds Send back. */
  const unreadable = createMemo(() =>
    chatStore.attachments().filter((note) => note.state === "unreadable"),
  );

  function closeMention() {
    if (debounce !== null) {
      clearTimeout(debounce);
      debounce = null;
    }
    setMention(null);
    setHits([]);
    setActive(0);
  }

  /** Fits the field to what it holds, up to the ceiling the stylesheet sets,
   * past which it scrolls rather than pushing the conversation off the top. */
  function fit(el: HTMLTextAreaElement) {
    el.style.height = "auto";
    el.style.height = `${el.scrollHeight}px`;
  }

  // The draft is also written from outside this component: a send clears it
  // and an edit fills it with a turn. The field follows it either way.
  createEffect(() => {
    chatStore.draft();
    if (input) fit(input);
  });

  // Edit is pressed on a turn, so the field takes the focus the press did
  // not give it, with the caret after the words the turn filled it with. The
  // store writes the draft before it names the turn, which is what puts the
  // caret after the whole of it rather than after the previous draft.
  createEffect(() => {
    if (chatStore.editing() === null || !input) return;
    const end = input.value.length;
    input.focus();
    input.setSelectionRange(end, end);
  });

  function onInput(event: InputEvent & { currentTarget: HTMLTextAreaElement }) {
    const el = event.currentTarget;
    chatStore.setDraft(el.value);
    fit(el);
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
    fit(input);
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
    // Escape answers the nearest thing first: the edit being written, then the
    // reply arriving, then the column itself.
    if (event.key === "Escape") {
      event.preventDefault();
      if (chatStore.editing() !== null) {
        chatStore.cancelEdit();
        return;
      }
      if (busy()) {
        chatStore.stop();
        return;
      }
      props.onClose();
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

      <Show when={chatStore.attachments().length > 0 || props.openNote() !== "none"}>
        <div class="chat-chips-row">
          <Show when={chatStore.attachments().length > 0}>
            <ul class="chat-chips" aria-label="Notes it can read">
              <For each={chatStore.attachments()}>
                {(note) => (
                  <li
                    class="chat-chip"
                    classList={{
                      "is-unreadable": note.state === "unreadable",
                      "is-dirty": note.dirty === true,
                    }}
                  >
                    <Icon name="file-text" size={12} />
                    <Tooltip label={chipTitle(note)}>
                      <span class="chat-chip-name">{chipLabel(note)}</span>
                    </Tooltip>
                    <span class="chat-chip-size">{byteLabel(note.bytes)}</span>
                    <button
                      type="button"
                      class="chat-chip-remove"
                      aria-label={`Remove ${noteName(note.path)}`}
                      onClick={() => chatStore.detach(note.path)}
                    >
                      <Icon name="x" size={10} />
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <Show when={props.openNote() !== "none"}>
            <button
              type="button"
              class="chat-chip-add"
              disabled={props.openNote() === "unsaved"}
              aria-describedby={props.openNote() === "unsaved" ? ADD_REASON_ID : undefined}
              onClick={() => void chatStore.addOpenNote()}
            >
              <Icon name="plus" size={12} />
              Add open note
            </button>
          </Show>
        </div>
      </Show>

      <Show when={props.openNote() === "unsaved"}>
        <p class="chat-composer-note" id={ADD_REASON_ID}>
          Save this note first
        </p>
      </Show>

      <For each={unreadable()}>
        {(note) => (
          <p class="chat-composer-note" role="status">
            {chipLabel(note)}: {note.reason ?? "This note could not be read."}
          </p>
        )}
      </For>

      <div class="chat-composer-field">
        <Show when={isOpen()}>
          <MentionPopover hits={hits()} active={active()} onPick={pick} />
        </Show>
        <textarea
          class="chat-composer-input"
          rows={2}
          spellcheck={false}
          placeholder={
            chatStore.attachments().length > 0
              ? "Ask about the attached notes. @ attaches another."
              : "@ attaches a note."
          }
          aria-label="Message"
          role="combobox"
          aria-autocomplete="list"
          aria-expanded={isOpen()}
          aria-controls={isOpen() ? MENTION_LIST_ID : undefined}
          aria-activedescendant={isOpen() && hits().length > 0 ? mentionRowId(active()) : undefined}
          ref={input}
          value={chatStore.draft()}
          onInput={onInput}
          onKeyDown={onKeyDown}
          onBlur={closeMention}
        />
      </div>

      <div class="chat-composer-actions">
        <ChatConnectionControl />
        <Show when={chatStore.editing() !== null}>
          <Button onClick={() => chatStore.cancelEdit()}>Cancel</Button>
        </Show>
        <Show
          when={busy()}
          fallback={
            <Button
              variant="primary"
              disabled={chatStore.draft().trim().length === 0 || unreadable().length > 0}
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
