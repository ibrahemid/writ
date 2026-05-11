import { onMount, onCleanup, createEffect } from "solid-js";
import TitleBar from "./components/TitleBar/TitleBar";
import EditorArea from "./components/Editor/EditorArea";
import Sidebar from "./components/Sidebar/Sidebar";
import CommandPalette, { toggleCommandPalette } from "./components/CommandPalette/CommandPalette";
import ContextMenu from "./components/ContextMenu/ContextMenu";
import ToastContainer, { showToast } from "./components/Notifications/Toast";
import ErrorBoundary from "./components/ErrorBoundary/ErrorBoundary";
import { bufferStore } from "./stores/buffers";
import { sidebarStore } from "./stores/sidebar";
import { editorStore } from "./stores/editor";
import { configStore } from "./stores/config";
import { focusSearchBar } from "./components/Sidebar/SearchBar";
import { registerCommand, executeCommand } from "./commands/registry";
import { installKeyboardHandler, rebuildKeyMap } from "./commands/keybindings";
import { onEvent } from "./services/events";
import { onAutosaveError } from "./services/autosave";
import { onDragDrop, consumePendingOpens } from "./services/tauri";
import type { UnlistenFn } from "./services/events";
import "./styles/global.css";
import "./App.css";

let processingPending = false;
async function processPendingOpens() {
  if (processingPending) return;
  processingPending = true;
  try {
    const pending = await consumePendingOpens();
    for (const path of pending) {
      try {
        await bufferStore.openFile(path);
      } catch {}
    }
  } finally {
    processingPending = false;
  }
}

export default function App() {
  let unlisteners: UnlistenFn[] = [];
  let pollTimer: ReturnType<typeof setInterval> | null = null;

  onMount(async () => {
    await configStore.load();
    sidebarStore.hydrateFromConfig();
    await bufferStore.load();

    await processPendingOpens();

    if (bufferStore.activeTabs().length === 0 && !bufferStore.activeTabId()) {
      await bufferStore.createTab();
    } else if (!bufferStore.activeTabId()) {
      const tabs = bufferStore.activeTabs();
      if (tabs.length > 0) {
        bufferStore.setActiveTabId(tabs[tabs.length - 1].id);
      }
    }

    registerCommand({
      id: "buffer.new",
      label: "New Tab",
      keybinding: "CmdOrCtrl+T",
      scope: "app",
      execute: () => bufferStore.createTab(),
    });

    registerCommand({
      id: "file.open",
      label: "Open File",
      keybinding: "CmdOrCtrl+O",
      scope: "app",
      execute: () => bufferStore.openFileDialog(),
    });

    registerCommand({
      id: "buffer.close",
      label: "Close Tab",
      keybinding: "CmdOrCtrl+W",
      scope: "app",
      execute: () => {
        const id = bufferStore.activeTabId();
        if (id) bufferStore.closeTab(id);
      },
    });

    registerCommand({
      id: "buffer.nextTab",
      label: "Next Tab",
      keybinding: "CmdOrCtrl+]",
      scope: "app",
      execute: () => {
        const tabs = bufferStore.activeTabs();
        const currentId = bufferStore.activeTabId();
        if (tabs.length < 2 || !currentId) return;
        const idx = tabs.findIndex(t => t.id === currentId);
        const nextIdx = (idx + 1) % tabs.length;
        bufferStore.setActiveTabId(tabs[nextIdx].id);
      },
    });

    registerCommand({
      id: "buffer.prevTab",
      label: "Previous Tab",
      keybinding: "CmdOrCtrl+[",
      scope: "app",
      execute: () => {
        const tabs = bufferStore.activeTabs();
        const currentId = bufferStore.activeTabId();
        if (tabs.length < 2 || !currentId) return;
        const idx = tabs.findIndex(t => t.id === currentId);
        const prevIdx = (idx - 1 + tabs.length) % tabs.length;
        bufferStore.setActiveTabId(tabs[prevIdx].id);
      },
    });

    registerCommand({
      id: "history.restoreLast",
      label: "Reopen Closed Tab",
      keybinding: "CmdOrCtrl+Shift+T",
      scope: "app",
      execute: () => {
        const history = bufferStore.historyList();
        if (history.length > 0) bufferStore.restoreFromHistory(history[0].id);
      },
    });

    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      execute: () => sidebarStore.toggle(),
    });

    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      keybinding: "Shift+Shift",
      scope: "app",
      execute: () => toggleCommandPalette(),
    });

    registerCommand({
      id: "buffer.closeAll",
      label: "Close All Tabs",
      scope: "app",
      execute: () => bufferStore.closeAllTabs(),
    });

    registerCommand({
      id: "history.clear",
      label: "Clear History",
      scope: "app",
      execute: () => bufferStore.clearAllHistory(),
    });

    rebuildKeyMap();
    installKeyboardHandler();

    const unlisten1 = await onEvent("config:changed", () => {
      configStore.load();
    });
    unlisteners.push(unlisten1);

    const unlisten2 = await onEvent("buffer:external", (payload) => {
      if (payload.bufferId && payload.change) {
        showToast(`File "${payload.bufferId}" ${payload.change} externally`, "warning");
      }
      bufferStore.load();
    });
    unlisteners.push(unlisten2);

    const unlisten3 = await onEvent("menu:action", (payload) => {
      executeCommand(payload.action);
    });
    unlisteners.push(unlisten3);

    const unlisten4 = await onDragDrop((event) => {
      if (event.type === "drop" && event.paths.length > 0) {
        for (const path of event.paths) {
          bufferStore.openFile(path).catch(() => {
            showToast(`Failed to open ${path}`, "error");
          });
        }
      }
    });
    unlisteners.push(unlisten4);

    pollTimer = setInterval(processPendingOpens, 500);

    const offAutosaveError = onAutosaveError((bufferId) => {
      showToast(`Autosave failed for ${bufferId}`, "error");
    });
    unlisteners.push(offAutosaveError);
  });

  onCleanup(() => {
    if (pollTimer) clearInterval(pollTimer);
    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  createEffect(() => {
    if (sidebarStore.isOpen()) {
      focusSearchBar();
    } else {
      editorStore.focusEditor();
    }
  });

  return (
    <ErrorBoundary>
      <div class="app-container">
        <TitleBar />
        <div class="app-body">
          <Sidebar />
          <EditorArea />
        </div>
        <CommandPalette />
        <ContextMenu />
        <ToastContainer />
      </div>
    </ErrorBoundary>
  );
}
