import { Show, createEffect, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import SearchBar from "../Sidebar/SearchBar";
import { useWindow } from "../WindowProvider/WindowProvider";
import { executeCommand } from "../../commands/registry";
import { CHAT_TOGGLE_COMMAND_ID } from "../../commands/chat";
import { configStore } from "../../stores/global/config";
import { useEffectiveBinding } from "../../commands/keybindings";
import { formatKeybinding } from "../../lib/keybinding-format";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout, toolbarLeadsLights } from "../../lib/window-chrome";
import "./Toolbar.css";

function tip(label: string, binding: string | undefined): string {
  const chord = formatKeybinding(binding);
  return chord ? `${label} ${chord}` : label;
}

export default function Toolbar() {
  const win = useWindow();
  // Read per mount: the platform layer is written once at boot (ADR-030), so a
  // reactive read would only cost a navigator lookup per render.
  const platform = resolvePlatform();
  const layout = resolveChromeLayout(platform);
  // A closed sidebar takes its head with it, so the bar itself has to leave the
  // window lights their inset. The class eases the padding on the sidebar's own
  // motion token, which keeps the first control clear of them throughout.
  const leadsLights = () => toolbarLeadsLights(platform, win.sidebar.isOpen());
  const [focusIndex, setFocusIndex] = createSignal(0);
  let barRef: HTMLDivElement | undefined;

  const chatControl = createMemo(() => configStore.config().ai.chat.enabled);

  /**
   * The roving stops: the search field keeps its own tab stop and its arrows,
   * so it never joins the bar's single tab stop.
   */
  function stops(): HTMLButtonElement[] {
    if (!barRef) return [];
    return Array.from(barRef.querySelectorAll<HTMLButtonElement>("button:not([disabled])"));
  }

  // One tab stop for the bar. Re-runs when the chat control comes or goes,
  // because a control that is not rendered is not a stop.
  createEffect(() => {
    chatControl();
    const items = stops();
    if (items.length === 0) return;
    const active = Math.min(focusIndex(), items.length - 1);
    items.forEach((el, index) => {
      el.tabIndex = index === active ? 0 : -1;
    });
  });

  function handleKeyDown(event: KeyboardEvent) {
    const step = event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
    if (step === 0) return;
    if (event.target instanceof HTMLInputElement) return;
    const items = stops();
    if (items.length === 0) return;
    event.preventDefault();
    const current = items.findIndex((el) => el === document.activeElement);
    const next = (Math.max(current, 0) + step + items.length) % items.length;
    setFocusIndex(next);
    items[next].focus();
  }

  return (
    <div
      ref={(el) => (barRef = el)}
      class="writ-toolbar"
      classList={{ "leads-lights": leadsLights() }}
      role="toolbar"
      aria-label="File actions"
      data-platform={platform}
      // The bar is the drag region on macOS, where the window has no title bar
      // of its own. `deep` so the wrappers and the gaps between controls move
      // the window too; the walk still stops at any button or input that
      // carries no attribute of its own, so a press on a control clicks it.
      data-tauri-drag-region={platform === "mac" ? "deep" : undefined}
      onKeyDown={handleKeyDown}
    >
      <Tooltip label={tip("Toggle sidebar", useEffectiveBinding("sidebar.toggle", "CmdOrCtrl+\\"))}>
        <Button
          variant="ghost"
          class="writ-toolbar-btn"
          icon="sidebar-simple"
          aria-label="Toggle sidebar"
          onClick={() => executeCommand("sidebar.toggle")}
        />
      </Tooltip>

      {/* GNOME merges compose into the header bar with the window title. */}
      <Show when={!layout.composeInChrome}>
        <Tooltip label={tip("New file", useEffectiveBinding("note.new", "CmdOrCtrl+N"))}>
          <Button
            variant="ghost"
            class="writ-toolbar-compose"
            icon="note-pencil"
            onClick={() => executeCommand("note.new")}
          >
            New file
          </Button>
        </Tooltip>

        <div class="writ-toolbar-divider" role="separator" aria-orientation="vertical" />
      </Show>

      <Tooltip label={tip("Connections", useEffectiveBinding("panel.toggle", "CmdOrCtrl+Shift+\\"))}>
        <Button
          variant="ghost"
          class="writ-toolbar-btn"
          icon="link-simple"
          aria-label="Connections"
          pressed={win.rightPanel.isOpen()}
          onClick={() => executeCommand("panel.toggle")}
        />
      </Tooltip>

      {/* The command is registered whether chat is on or not, so the shortcut
          editor lists it and the View menu routes somewhere. The button is
          here only while the pane it opens exists. */}
      <Show when={configStore.config().ai.chat.enabled}>
        <Tooltip label={tip("Chat", useEffectiveBinding(CHAT_TOGGLE_COMMAND_ID, undefined))}>
          <Button
            variant="ghost"
            class="writ-toolbar-btn"
            icon="chat-text"
            aria-label="Chat"
            pressed={win.chatPanel.isOpen()}
            onClick={() => executeCommand(CHAT_TOGGLE_COMMAND_ID)}
          />
        </Tooltip>
      </Show>

      {/* GNOME keeps search in the sidebar's own header segment. */}
      <Show when={!layout.headerBar}>
        <SearchBar />
      </Show>
    </div>
  );
}
