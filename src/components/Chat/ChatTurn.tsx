import { For, Index, Show, createEffect, onCleanup } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import ProposalCard from "./ProposalCard";
import { providerLabel } from "./ChatTranscript";
import { linkStore } from "../../stores/global/link";
import {
  chatStore,
  noteName,
  type ChatDroppedProposal,
  type Message,
} from "../../stores/global/chat";

/** Why a block the reply wrote is not on offer, in the reader's words. The
 * note it named is stated; none of its text is (ADR-031 rule 5.2). */
export function dropLine(drop: ChatDroppedProposal): string {
  const named = drop.named.trim();
  const subject = named ? `An offer for ${named}` : "An offer";
  switch (drop.reason) {
    case "unknown_note":
      return `${subject} was dropped: that note is not attached.`;
    case "ambiguous_note":
      return `${subject} was dropped: more than one attached note has that name.`;
    case "truncated":
      return `${subject} was dropped: the reply ended before the note did.`;
    case "unterminated_block":
      return `${subject} was dropped: its text was never closed.`;
    case "empty_body":
      return `${subject} was dropped: it held no text.`;
    case "placeholder":
      return `${subject} was dropped: it repeated the example instead of the note.`;
    case "duplicate":
      return `${subject} was dropped: the same note was offered twice.`;
    default:
      return `${subject} was dropped.`;
  }
}

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

  /** The fragment this element already holds. A stream renders the same string
   * over and over, and writing it again would destroy and rebuild every node
   * in the reply for nothing. */
  let applied: string | null = null;

  // Scoped to this turn's own element, which is what the ref is for: nothing
  // here reaches the document.
  createEffect(() => {
    const el = reply;
    const html = props.message.html;
    if (!el || html === applied) return;
    applied = html;
    el.innerHTML = html;
    for (const block of Array.from(el.querySelectorAll("pre"))) {
      if (!block.querySelector("code")) continue;
      // The button sits beside the block rather than in it: a block wider than
      // the column scrolls sideways, and a button inside would scroll out.
      const box = document.createElement("div");
      box.className = "chat-code";
      block.replaceWith(box);
      box.append(block);
      const button = document.createElement("button");
      button.type = "button";
      button.className = "chat-code-copy";
      button.dataset.copy = "";
      button.textContent = "Copy";
      box.append(button);
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
      const source = copy.closest(".chat-code")?.querySelector("code")?.textContent ?? "";
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
        {/* By position, so a card keeps what its own write answered when the
            conversation is rebuilt around it. */}
        <Index each={props.message.proposals}>
          {(proposal) => <ProposalCard turn={props.message.turn} proposal={proposal()} />}
        </Index>
        <Show when={props.message.truncated}>
          <p class="chat-note">Reply was cut off.</p>
        </Show>
        <For each={props.message.dropped}>
          {(drop) => <p class="chat-note">{dropLine(drop)}</p>}
        </For>
        <Show when={props.message.identity}>
          {(identity) => (
            <p class="chat-identity">
              {identity().model} via {providerLabel(identity().provider)}
            </p>
          )}
        </Show>
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
