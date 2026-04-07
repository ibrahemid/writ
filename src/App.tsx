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
import { registerCommand } from "./commands/registry";
import { installKeyboardHandler, rebuildKeyMap } from "./commands/keybindings";
import { onEvent } from "./services/events";
import type { UnlistenFn } from "@tauri-apps/api/event";
import "./styles/global.css";
import "./App.css";

export default function App() {
  let unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    await configStore.load();
    await bufferStore.load();

    if (bufferStore.activeTabs().length === 0) {
      await bufferStore.createTab();
    }

    registerCommand({
      id: "buffer.new",
      label: "New Tab",
      keybinding: "CmdOrCtrl+T",
      scope: "app",
      execute: () => bufferStore.createTab(),
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
      keybinding: "CmdOrCtrl+double+S",
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

    const unlisten1 = await onEvent("config:changed", (payload) => {
      console.log("config changed:", payload.keys);
      configStore.load();
      showToast("Config reloaded", "info");
    });
    unlisteners.push(unlisten1);

    const unlisten2 = await onEvent("buffer:external", (payload) => {
      console.log("buffer external change:", payload);
      if (payload.bufferId && payload.change) {
        showToast(`File "${payload.bufferId}" ${payload.change} externally`, "warning");
      }
      bufferStore.load();
    });
    unlisteners.push(unlisten2);
  });

  onCleanup(() => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  createEffect(() => {
    if (sidebarStore.isVisible()) {
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
