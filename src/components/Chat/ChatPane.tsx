import { Show, createEffect, createMemo, createSignal, untrack } from "solid-js";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import EdgeResizer from "../Resizer/EdgeResizer";
import ChatComposer, { type OpenNoteState } from "./ChatComposer";
import ChatReadiness from "./ChatReadiness";
import ChatTranscript from "./ChatTranscript";
import ConversationList from "./ConversationList";
import { useWindow } from "../WindowProvider/WindowProvider";
import { bufferRegistry } from "../../stores/global/buffer-registry";
import {
  configStore,
  CHAT_WIDTH_MAX,
  CHAT_WIDTH_MIN,
  CHAT_WIDTH_DEFAULT,
} from "../../stores/global/config";
import { chatStore, noteName, type Attachment, type FrontTab } from "../../stores/global/chat";
import "./ChatPane.css";

/**
 * The chat column: what the model can read, what has been said, and what it
 * has offered to change.
 *
 * The conversation is a file Rust owns (ADR-040 section 8), so this column is
 * a view of one: opening it reads the chat last written to rather than
 * starting a blank one. Nothing here writes a note. Applying a proposal is a
 * person's click that goes through the guarded write like every other
 * (ADR-031 rule 4.3).
 */
export default function ChatPane() {
  const win = useWindow();

  // Non-null only while a drag is in flight: the edge follows the pointer
  // without a disk write per frame, and release commits the settled width.
  const [dragWidth, setDragWidth] = createSignal<number | null>(null);
  const width = () => dragWidth() ?? win.chatPanel.width();
  const [listing, setListing] = createSignal(false);
  const [renamingTitle, setRenamingTitle] = createSignal(false);

  const isOpen = () => win.chatPanel.isOpen();
  const notesInFront = createMemo(() =>
    bufferRegistry.activeTabs().filter((doc) => doc.source_path !== null),
  );

  /** Whether the note in front can be attached, and why it cannot: a tab with
   * no file has nothing on disk for a reply to read. */
  const openNote = (): OpenNoteState => {
    const front = frontTab();
    if (!front) return "none";
    return front.note ? "ready" : "unsaved";
  };

  /** The tab the editor is showing, with the note it holds. */
  const frontTab = createMemo<FrontTab | null>(() => {
    const id = win.tabs.activeTabId();
    if (!id) return null;
    const doc = notesInFront().find((tab) => tab.id === id);
    const note: Attachment | null = doc?.source_path
      ? { path: doc.source_path, name: noteName(doc.source_path), bytes: doc.size_bytes }
      : null;
    return { id, note };
  });

  // The note in front follows the editor: the automatic chip names whichever
  // tab is active, and an emptied set (a new chat, another conversation) asks
  // for it again. Every other chip is a person's and is left where it is. The
  // call is untracked because the store writes the list this effect reads.
  createEffect(() => {
    if (!isOpen()) return;
    chatStore.attachGeneration();
    const front = frontTab();
    untrack(() => void chatStore.followTab(front));
  });

  // One load per open, not one per reactive read: the effect tracks the open
  // flag and nothing else it would re-run for.
  let loaded = false;
  createEffect(() => {
    if (!isOpen()) {
      loaded = false;
      setListing(false);
      setRenamingTitle(false);
      return;
    }
    if (loaded) return;
    loaded = true;
    void chatStore.openPane();
  });

  // `inert` is presence-based, so it is set as an attribute rather than left
  // to the property: a closed column keeps its controls out of the tab order
  // at zero width, and an open one carries no attribute at all.
  let column: HTMLElement | undefined;
  createEffect(() => {
    column?.toggleAttribute("inert", !isOpen());
  });

  const title = () => chatStore.current()?.title ?? "Chat";

  function commitTitle(next: string) {
    setRenamingTitle(false);
    const id = chatStore.current()?.id;
    const trimmed = next.trim();
    if (!id || !trimmed || trimmed === title()) return;
    void chatStore.rename(id, trimmed);
  }

  return (
    <Show when={configStore.config().ai.chat.enabled}>
      <aside
        class="chat-pane"
        classList={{ "is-open": isOpen(), "is-resizing": dragWidth() !== null }}
        style={{ "--writ-chat-live-width": `${width()}px` }}
        aria-labelledby="chat-pane-title"
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
            <Show
              when={renamingTitle()}
              fallback={
                <h2
                  class="chat-pane-title"
                  id="chat-pane-title"
                  onDblClick={() => {
                    if (chatStore.current()) setRenamingTitle(true);
                  }}
                >
                  {title()}
                </h2>
              }
            >
              <input
                class="chat-pane-rename"
                value={title()}
                aria-label="Chat name"
                ref={(el) => queueMicrotask(() => el.select())}
                onBlur={(event) => commitTitle(event.currentTarget.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter") {
                    event.preventDefault();
                    commitTitle(event.currentTarget.value);
                  } else if (event.key === "Escape") {
                    event.preventDefault();
                    setRenamingTitle(false);
                  }
                }}
              />
            </Show>

            <Tooltip label="Chats">
              <Button
                variant="ghost"
                icon="list-bullets"
                iconSize={14}
                aria-label="Chats"
                pressed={listing()}
                onClick={() => {
                  const next = !listing();
                  setListing(next);
                  if (next) void chatStore.refreshList();
                }}
              />
            </Tooltip>
            <Tooltip label="New chat">
              <Button
                variant="ghost"
                icon="note-pencil"
                iconSize={14}
                aria-label="New chat"
                onClick={() => {
                  chatStore.newChat();
                  setListing(false);
                }}
              />
            </Tooltip>
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

          <Show when={listing()}>
            <ConversationList onPick={() => setListing(false)} />
          </Show>

          <ChatTranscript />
          <ChatReadiness />
          <ChatComposer openNote={openNote} onClose={() => win.chatPanel.hide()} />
        </div>
      </aside>
    </Show>
  );
}
