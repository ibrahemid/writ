import { For, Show, createEffect, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Icon from "../Icon/Icon";
import Tooltip from "../Tooltip/Tooltip";
import EdgeResizer from "../Resizer/EdgeResizer";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import { configStore, CHAT_WIDTH_MAX, CHAT_WIDTH_MIN, CHAT_WIDTH_DEFAULT } from "../../stores/global/config";
import {
  chatStore,
  type Attachment,
  type ChatProposal,
  type Message,
} from "../../stores/global/chat";
import { byteLabel, sendChatMessage } from "../../commands/chat";
import "./ChatPane.css";

/** The note's own name, which is what a row shows. */
function noteName(path: string): string {
  const parts = path.split(/[\\/]/);
  return parts[parts.length - 1] || path;
}

/**
 * The chat column: what the model can read, what has been said, and what it
 * has offered to change.
 *
 * The attached notes sit at the top and stay there, because they are the whole
 * of what leaves the machine: the list is the answer to "what can it see"
 * (ADR-031 rule 2.5). Nothing here writes a note. A reply that offers a change
 * renders it beside the note's current text, and applying is a person's click
 * that goes through the guarded write like every other (rule 4.3).
 */
export default function ChatPane() {
  const win = useWindow();

  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);
  const width = () => dragWidth() ?? win.chatPanel.width();
  const [picking, setPicking] = createSignal(false);

  const isOpen = () => win.chatPanel.isOpen();
  const notesInFront = createMemo(() =>
    bufferRegistry.activeTabs().filter((doc) => doc.source_path !== null),
  );

  /** The note in front, as an attachment. */
  function frontNote(): Attachment | null {
    const id = win.tabs.activeTabId();
    const doc = notesInFront().find((tab) => tab.id === id);
    if (!doc?.source_path) return null;
    return { path: doc.source_path, name: noteName(doc.source_path), bytes: doc.size_bytes };
  }

  // Opening attaches the note in front and nothing else. Every later change to
  // the list is a person's: a tab switch does not quietly add a note to what
  // the next message carries.
  createEffect(() => {
    if (!isOpen()) return;
    if (chatStore.attachments().length > 0) return;
    const front = frontNote();
    if (front) chatStore.attach(front);
  });

  const attachable = createMemo(() =>
    notesInFront()
      .filter((doc) => doc.source_path && !chatStore.isAttached(doc.source_path))
      .map((doc) => ({
        path: doc.source_path as string,
        name: noteName(doc.source_path as string),
        bytes: doc.size_bytes,
      })),
  );

  // The newest turn is the one being read. Streamed text appends into the last
  // message, so this follows it down; it moves the scroll position and nothing
  // else, which is what a reduced-motion setting asks of a live region.
  let transcript: HTMLDivElement | undefined;
  createEffect(() => {
    const messages = chatStore.messages();
    // Read so a frame that only lengthens the last message still moves it.
    chatStore.status();
    if (!transcript || messages.length === 0) return;
    transcript.scrollTop = transcript.scrollHeight;
  });

  // `inert` is presence-based, so it is set as an attribute rather than left
  // to the property: a closed column keeps its controls out of the tab order
  // at zero width, and an open one carries no attribute at all.
  let column: HTMLElement | undefined;
  createEffect(() => {
    column?.toggleAttribute("inert", !isOpen());
  });

  createEffect(() => {
    if (!isOpen()) setPicking(false);
  });

  function onComposerKeyDown(event: KeyboardEvent) {
    if (event.key !== "Enter" || event.shiftKey) return;
    event.preventDefault();
    void sendChatMessage();
  }

  return (
    <Show when={configStore.config().ai.chat.enabled}>
      <aside
        class="chat-pane"
        classList={{ "is-open": isOpen(), "is-resizing": dragWidth() !== null }}
        style={{ "--writ-chat-live-width": `${width()}px` }}
        aria-label="Chat"
        aria-hidden={isOpen() ? undefined : "true"}
        ref={column}
      >
        <EdgeResizer
          class="chat-pane-resizer"
          label="Chat width"
          width={() => win.chatPanel.width()}
          min={CHAT_WIDTH_MIN}
          max={CHAT_WIDTH_MAX}
          direction={-1}
          onDrag={setDragWidth}
          onCommit={(next) => win.chatPanel.setWidth(next)}
          onReset={() => win.chatPanel.setWidth(CHAT_WIDTH_DEFAULT)}
        />

        <div class="chat-pane-inner">
          <header class="chat-pane-header">
            <h2 class="chat-pane-title">Chat</h2>
            <Tooltip label="Close chat">
              <Button
                variant="ghost"
                icon="x"
                iconSize={14}
                aria-label="Close chat"
                onClick={() => win.chatPanel.hide()}
              />
            </Tooltip>
          </header>

          <section class="chat-attached" aria-label="Attached notes">
            <h3 class="chat-section-title">Notes it can read</h3>
            <Show
              when={chatStore.attachments().length > 0}
              fallback={<p class="chat-empty">No notes attached. Nothing is sent yet.</p>}
            >
              <ul class="chat-attached-list">
                <For each={chatStore.attachments()}>
                  {(note) => (
                    <li class="chat-attached-row">
                      <Icon name="file-text" size={14} />
                      <span class="chat-attached-name">{note.name}</span>
                      <span class="chat-attached-size">{byteLabel(note.bytes)}</span>
                      <Button
                        variant="ghost"
                        icon="minus"
                        iconSize={12}
                        aria-label={`Remove ${note.name}`}
                        onClick={() => chatStore.detach(note.path)}
                      />
                    </li>
                  )}
                </For>
              </ul>
            </Show>

            <Button
              variant="ghost"
              icon="plus"
              iconSize={12}
              pressed={picking()}
              onClick={() => setPicking((open) => !open)}
            >
              Attach a note
            </Button>

            <Show when={picking()}>
              <Show
                when={attachable().length > 0}
                fallback={<p class="chat-empty">Every open note is attached.</p>}
              >
                <ul class="chat-attached-list">
                  <For each={attachable()}>
                    {(note) => (
                      <li>
                        <button
                          type="button"
                          class="chat-pick-row"
                          onClick={() => {
                            chatStore.attach(note);
                            setPicking(false);
                          }}
                        >
                          <Icon name="file-text" size={14} />
                          <span class="chat-attached-name">{note.name}</span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
            </Show>
          </section>

          <div class="chat-transcript" ref={transcript}>
            <Show
              when={chatStore.messages().length > 0}
              fallback={
                <p class="chat-empty">
                  Ask about the notes above. Answers can offer a change, which you read and apply
                  yourself.
                </p>
              }
            >
              <For each={chatStore.messages()}>
                {(message) => <Turn message={message} />}
              </For>
            </Show>
            <Show when={chatStore.status() === "error"}>
              <p class="chat-error" role="alert">
                {chatStore.errorMessage()}
              </p>
            </Show>
          </div>

          <div class="chat-composer">
            <textarea
              class="chat-composer-input"
              rows={3}
              spellcheck={false}
              placeholder="Ask about the attached notes"
              aria-label="Message"
              value={chatStore.draft()}
              onInput={(event) => chatStore.setDraft(event.currentTarget.value)}
              onKeyDown={onComposerKeyDown}
            />
            <div class="chat-composer-actions">
              <Show
                when={chatStore.status() === "streaming"}
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
        </div>
      </aside>
    </Show>
  );
}

/** One turn. A person's is filled; a reply sits on the pane's own ground. */
function Turn(props: { message: Message }) {
  return (
    <article class="chat-turn" classList={{ "is-mine": props.message.role === "user" }}>
      <Show when={props.message.content.length > 0}>
        <p class="chat-turn-text">{props.message.content}</p>
      </Show>
      <For each={props.message.proposals}>
        {(proposal) => (
          <ProposalCard
            proposal={proposal}
            before={
              props.message.context.find((note) => note.path === proposal.path)?.text ?? ""
            }
            verdict={props.message.decided[proposal.path]}
            refusal={props.message.refusal[proposal.path]}
          />
        )}
      </For>
    </article>
  );
}

/** One offered change, read beside the text the model was given. */
function ProposalCard(props: {
  proposal: ChatProposal;
  before: string;
  verdict?: "applied" | "discarded" | "refused";
  refusal?: string;
}) {
  return (
    <section class="chat-proposal" aria-label={`Change to ${props.proposal.path}`}>
      <header class="chat-proposal-header">
        <Icon name="file-text" size={14} />
        <span class="chat-attached-name">{props.proposal.path}</span>
      </header>
      <Show when={props.proposal.summary.length > 0}>
        <p class="chat-proposal-summary">{props.proposal.summary}</p>
      </Show>
      <div class="chat-proposal-panes">
        <div class="chat-proposal-pane">
          <div class="chat-proposal-label">Now</div>
          <div class="chat-proposal-body" data-pane="before">
            {props.before}
          </div>
        </div>
        <div class="chat-proposal-pane">
          <div class="chat-proposal-label">Proposed</div>
          <div class="chat-proposal-body chat-proposal-new" data-pane="after">
            {props.proposal.new_content}
          </div>
        </div>
      </div>
      <Show
        when={props.verdict === undefined}
        fallback={
          <p class="chat-proposal-verdict" role="status">
            {verdictLine(props)}
          </p>
        }
      >
        <div class="chat-proposal-actions">
          <Button onClick={() => void chatStore.discard(props.proposal)}>Discard</Button>
          <Button variant="primary" onClick={() => void chatStore.apply(props.proposal)}>
            Apply
          </Button>
        </div>
      </Show>
    </section>
  );
}

function verdictLine(props: { verdict?: string; refusal?: string }): string {
  if (props.verdict === "applied") return "Applied.";
  if (props.verdict === "discarded") return "Discarded.";
  return props.refusal ?? "Not applied.";
}
