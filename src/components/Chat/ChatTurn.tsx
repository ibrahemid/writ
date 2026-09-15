import { For, Show, createEffect, onCleanup } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import ProposalCard from "./ProposalCard";
import { linkStore } from "../../stores/global/link";
import { chatStore, noteName, type Message } from "../../stores/global/chat";

/** How long a copy button reads "Copied" before going back to its label. */
const COPIED_MS = 1200;

/** The schemes the renderer keeps in an untrusted reply (`WEB_SCHEMES` in
 * `writ-render`), which are the same three Writ's external path opens. A link
 * of any other scheme was already dropped, and a click on one does nothing. */
function isExternal(href: string): boolean {
  return /^(https?|mailto):/i.test(href);
}

/**
 * One turn. A person's is filled; a reply sits on the pane's own ground.
 *
 * A reply arrives as a fragment from `chat_render_reply`, which runs the
 * untrusted variant: raw HTML in model output is dropped rather than escaped
 * (ADR-040 section 9). The fragment goes into this element, never into the
 * preview iframe, which ADR-031 rule 1.6 keeps untouched.
 */
export default function ChatTurn(props: { message: Message; thinking: boolean }) {
  let reply: HTMLDivElement | undefined;
  let copiedTimer: ReturnType<typeof setTimeout> | null = null;

  onCleanup(() => {
    if (copiedTimer !== null) clearTimeout(copiedTimer);
  });

  // Scoped to this turn's own element, which is what the ref is for: nothing
  // here reaches the document.
  createEffect(() => {
    const el = reply;
    const html = props.message.html;
    if (!el) return;
    el.innerHTML = html;
    for (const block of Array.from(el.querySelectorAll("pre"))) {
      if (!block.querySelector("code")) continue;
      block.classList.add("chat-code");
      const button = document.createElement("button");
      button.type = "button";
      button.className = "chat-code-copy";
      button.dataset.copy = "";
      button.textContent = "Copy";
      block.append(button);
    }
    // A table sets its own width from its content, which beats the column's,
    // so it scrolls in a box of its own rather than widening the pane.
    for (const table of Array.from(el.querySelectorAll("table"))) {
      const box = document.createElement("div");
      box.className = "chat-reply-table";
      table.replaceWith(box);
      box.append(table);
    }
  });

  // One handler for the whole fragment, so replacing its HTML leaves no
  // listener behind.
  function onReplyClick(event: MouseEvent) {
    const target = event.target;
    if (!(target instanceof Element)) return;

    const copy = target.closest<HTMLElement>("[data-copy]");
    if (copy) {
      const source = copy.closest("pre")?.querySelector("code")?.textContent ?? "";
      void chatStore.copyCode(source).then((landed) => {
        if (landed) sayCopied(copy);
      });
      return;
    }

    const link = target.closest<HTMLAnchorElement>("a[href]");
    if (!link) return;
    // A reply is model output, so a click never navigates the window. A web
    // target opens where every other external link opens, and anything else
    // does nothing.
    event.preventDefault();
    const href = link.getAttribute("href") ?? "";
    if (isExternal(href)) void linkStore.openExternal(href);
  }

  function sayCopied(button: HTMLElement) {
    if (copiedTimer !== null) clearTimeout(copiedTimer);
    button.textContent = "Copied";
    copiedTimer = setTimeout(() => {
      copiedTimer = null;
      if (button.isConnected) button.textContent = "Copy";
    }, COPIED_MS);
  }

  const mine = () => props.message.role === "user";

  return (
    <article class="chat-turn" classList={{ "is-mine": mine() }}>
      <Show when={mine()}>
        <div class="chat-turn-mine">
          <p class="chat-turn-text">{props.message.content}</p>
          <Show when={props.message.attachments.length > 0}>
            <ul class="chat-turn-notes">
              <For each={props.message.attachments}>
                {(note) => (
                  <li class="chat-turn-note">
                    <Icon name="file-text" size={12} />
                    {noteName(note.path)}
                  </li>
                )}
              </For>
            </ul>
          </Show>
          <div class="chat-turn-actions">
            <Button
              variant="ghost"
              icon="pencil-simple"
              iconSize={12}
              onClick={() => void chatStore.beginEdit(props.message.turn)}
            >
              Edit
            </Button>
          </div>
        </div>
      </Show>

      <Show when={!mine()}>
        <Show when={props.thinking}>
          <p class="chat-thinking" role="status">
            <span class="chat-thinking-dots" aria-hidden="true">
              <span />
              <span />
              <span />
            </span>
            Thinking
          </p>
        </Show>
        <Show when={props.message.html.length > 0} fallback={<RawReply text={props.message.content} />}>
          <div class="chat-reply" ref={reply} onClick={onReplyClick} />
        </Show>
        <For each={props.message.proposals}>
          {(proposal) => <ProposalCard turn={props.message.turn} proposal={proposal} />}
        </For>
      </Show>
    </article>
  );
}

/** The reply as it arrives, before the first render comes back. */
function RawReply(props: { text: string }) {
  return (
    <Show when={props.text.length > 0}>
      <p class="chat-turn-text">{props.text}</p>
    </Show>
  );
}
