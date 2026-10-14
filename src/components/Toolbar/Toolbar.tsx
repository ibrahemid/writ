import { Show, createEffect, createMemo, createSignal } from "solid-js";
import Button from "../Button/Button";
import Tooltip from "../Tooltip/Tooltip";
import SearchBar from "../Sidebar/SearchBar";
import { useWindow } from "../WindowProvider/WindowProvider";
import { executeCommand } from "../../commands/registry";
import { CHAT_TOGGLE_COMMAND_ID } from "../../commands/chat";
import { configStore } from "../../stores/global/config";
import { activityStore } from "../../stores/global/activity";
import { resolvePlatform } from "../../lib/platform";
import { resolveChromeLayout, toolbarLeadsLights } from "../../lib/window-chrome";
import "./Toolbar.css";

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

  const appControls = createMemo(
    () =>
      `${configStore.isAppOn("connections")}:${configStore.isAppOn("graph")}:${configStore.isAppOn("chat")}:${configStore.isAppOn("programs")}`,
  );

  const waitingCount = () => activityStore.pending().length;
  const programsLabel = () => {
    const waiting = waitingCount();
    if (waiting === 0) return "Program activity";
    return waiting === 1
      ? "Program activity, 1 program waiting"
      : `Program activity, ${waiting} programs waiting`;
  };

  /**
   * The roving stops: the search field keeps its own tab stop and its arrows,
   * so it never joins the bar's single tab stop.
   */
  function stops(): HTMLButtonElement[] {
    if (!barRef) return [];
    return Array.from(barRef.querySelectorAll<HTMLButtonElement>("button:not([disabled])"));
  }

  // One tab stop for the bar. Re-runs when an app's control comes or goes,
  // because a control that is not rendered is not a stop.
  createEffect(() => {
    appControls();
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
      <Tooltip label="Toggle sidebar" kbd="sidebar.toggle">
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
        <Tooltip label="New file" kbd="note.new">
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

      <Show when={configStore.isAppOn("connections")}>
        <Tooltip label="Connections" kbd="panel.toggle">
          <Button
            variant="ghost"
            class="writ-toolbar-btn"
            icon="link-simple"
            aria-label="Connections"
            pressed={win.rightPanel.isOpen()}
            onClick={() => executeCommand("panel.toggle")}
          />
        </Tooltip>
      </Show>

      {/* Each button is here only while its app is on (ADR-042 section 3). */}
      <Show when={configStore.isAppOn("graph")}>
        <Tooltip label="Graph" kbd="folderGraph.open">
          <Button
            variant="ghost"
            class="writ-toolbar-btn"
            icon="graph"
            aria-label="Graph"
            pressed={win.folderGraph.isOpen()}
            onClick={() => win.folderGraph.toggle()}
          />
        </Tooltip>
      </Show>

      <Show when={configStore.isAppOn("chat")}>
        <Tooltip label="Chat" kbd={CHAT_TOGGLE_COMMAND_ID}>
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

      <Show when={configStore.isAppOn("programs")}>
        <Tooltip label={programsLabel()}>
          <Button
            variant="ghost"
            class="writ-toolbar-btn writ-toolbar-programs"
            icon="plugs-connected"
            aria-label={programsLabel()}
            onClick={() => executeCommand("activity.open")}
          >
            <Show when={waitingCount() > 0}>
              <span class="writ-toolbar-dot" data-waiting aria-hidden="true" />
            </Show>
          </Button>
        </Tooltip>
      </Show>

      {/* GNOME keeps search in the sidebar's own header segment. */}
      <Show when={!layout.headerBar}>
        <SearchBar />
      </Show>
    </div>
  );
}
