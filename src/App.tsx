import { onMount, onCleanup, createEffect } from "solid-js";
import TitleBar from "./components/TitleBar/TitleBar";
import EditorArea from "./components/Editor/EditorArea";
import Sidebar from "./components/Sidebar/Sidebar";
import CommandPalette, { toggleCommandPalette } from "./components/CommandPalette/CommandPalette";
import ThemeEditor, { openThemeEditor } from "./components/ThemeEditor/ThemeEditor";
import ShortcutEditor, { openShortcutEditor } from "./components/ShortcutEditor/ShortcutEditor";
import { startRenameActiveTab } from "./components/Editor/TabBar";
import ContextMenu from "./components/ContextMenu/ContextMenu";
import ToastContainer, { showToast } from "./components/Notifications/Toast";
import ConfirmDialog, { requestConfirm } from "./components/ConfirmDialog/ConfirmDialog";
import ErrorBoundary from "./components/ErrorBoundary/ErrorBoundary";
import UpdateBanner from "./components/UpdateBanner/UpdateBanner";
import { bufferStore } from "./stores/buffers";
import { updateStore } from "./stores/update";
import { sidebarStore } from "./stores/sidebar";
import { editorStore } from "./stores/editor";
import { configStore } from "./stores/config";
import { themeStore } from "./stores/theme";
import { focusSearchBar } from "./components/Sidebar/SearchBar";
import { openContentSearch } from "./commands/search";
import { registerTransformCommands } from "./commands/transforms";
import { registerCommand, executeCommand, getAllCommands, setExecuteListener } from "./commands/registry";
import {
  installKeyboardHandler,
  uninstallKeyboardHandler,
  rebuildKeyMap,
  setKeybindingOverrides,
  pruneLegacyDefaultOverrides,
} from "./commands/keybindings";
import { onEvent, emitFrontendReady } from "./services/events";
import { onAutosaveError } from "./services/autosave";
import { reportFirstPaint } from "./services/tauri";
import { osWindowStore } from "./stores/os-window";
import { installCloseFlush } from "./services/window-lifecycle";
import type { UnlistenFn } from "./services/events";
import "./styles/global.css";
import "./App.css";

async function openPendingPaths(paths: string[]) {
  if (!Array.isArray(paths)) {
    console.error("openPendingPaths: expected string[], got", paths);
    return;
  }
  for (const path of paths) {
    try {
      await bufferStore.openFile(path);
    } catch (err) {
      console.error("openPendingPaths: failed to open", path, err);
    }
  }
}

function measureFirstPaint(
  mode: "cold" | "warm",
  rustElapsedUs: number | null = null,
) {
  const start = performance.now();
  requestAnimationFrame(() => {
    const elapsed = performance.now() - start;
    void reportFirstPaint(elapsed, mode, rustElapsedUs);
  });
}

export default function App() {
  let unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    measureFirstPaint("cold");
    themeStore.applyToRoot();
    await configStore.load();
    themeStore.loadConfig(configStore.config().theme);
    await osWindowStore.restoreSize();
    const offFocusSync = await osWindowStore.installFocusSync();
    unlisteners.push(offFocusSync);
    const offWindowResize = await osWindowStore.installSizePersistence();
    unlisteners.push(offWindowResize);
    const offCloseFlush = await installCloseFlush();
    unlisteners.push(offCloseFlush);
    sidebarStore.hydrateFromConfig();
    await bufferStore.load();

    const unlistenPending = await onEvent("pending:opens", (payload) => {
      void openPendingPaths(payload.paths);
    });
    unlisteners.push(unlistenPending);

    const unlistenShown = await onEvent("window:shown", (payload) => {
      measureFirstPaint("warm", payload.rust_elapsed_us);
    });
    unlisteners.push(unlistenShown);

    await emitFrontendReady();

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
      description: "Create a new empty buffer",
      keybinding: "CmdOrCtrl+T",
      scope: "app",
      execute: () => bufferStore.createTab(),
    });

    registerCommand({
      id: "file.open",
      label: "Open File",
      description: "Open a file from disk into a new tab",
      keybinding: "CmdOrCtrl+O",
      scope: "app",
      execute: () => bufferStore.openFileDialog(),
    });

    registerCommand({
      id: "buffer.close",
      label: "Close Tab",
      description: "Close the active tab",
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
      description: "Cycle to the next open tab",
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
      description: "Cycle to the previous open tab",
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
      description: "Restore the most recently closed tab",
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
      description: "Show or hide the tabs + history rail",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      execute: () => sidebarStore.toggle(),
    });

    registerCommand({
      id: "search.openContent",
      label: "Search content…",
      scope: "app",
      execute: openContentSearch,
    });

    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      description: "Search and run any command",
      keybinding: "Shift+Shift",
      scope: "app",
      execute: () => toggleCommandPalette(),
    });

    registerCommand({
      id: "tab.rename",
      label: "Rename Tab",
      description: "Rename the active tab",
      keybinding: "CmdOrCtrl+R",
      keybindingAliases: ["F2", "CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => startRenameActiveTab(),
    });

    registerCommand({
      id: "buffer.closeAll",
      label: "Close All Tabs",
      description: "Move every open tab into history",
      scope: "app",
      execute: async () => {
        const tabs = bufferStore.activeTabs();
        if (tabs.length === 0) return;
        const confirmed = await requestConfirm({
          title: "Close all tabs?",
          message: `All ${tabs.length} open tab${tabs.length === 1 ? "" : "s"} will move to history. You can reopen them from the sidebar.`,
          confirmLabel: "Close all",
        });
        if (confirmed) bufferStore.closeAllTabs();
      },
    });

    registerCommand({
      id: "history.clear",
      label: "Clear History",
      description: "Permanently remove all history entries",
      scope: "app",
      execute: async () => {
        const count = bufferStore.historyList().length;
        if (count === 0) return;
        const confirmed = await requestConfirm({
          title: "Clear all history?",
          message: `This permanently removes ${count} closed tab${count === 1 ? "" : "s"} from history. This cannot be undone.`,
          confirmLabel: "Clear history",
          danger: true,
        });
        if (confirmed) bufferStore.clearAllHistory();
      },
    });

    registerCommand({
      id: "theme.customize",
      label: "Customize theme…",
      description: "Switch presets or override individual colors live",
      scope: "app",
      execute: () => openThemeEditor(),
    });

    registerCommand({
      id: "commands.clearUsage",
      label: "Clear command usage history",
      description: "Forget which commands you have used and how often",
      scope: "app",
      execute: async () => {
        const confirmed = await requestConfirm({
          title: "Clear command usage history?",
          message: "Recent and frequently-used ordering will reset to default.",
          confirmLabel: "Clear",
        });
        if (!confirmed) return;
        configStore.clearCommandUsage().then(
          () => showToast("Command usage cleared", "success"),
          () => showToast("Failed to clear command usage", "error"),
        );
      },
    });

    registerCommand({
      id: "shortcuts.customize",
      label: "Customize shortcuts…",
      description: "Rebind any command in the palette",
      scope: "app",
      execute: () => openShortcutEditor(),
    });

    registerCommand({
      id: "app.check_updates",
      label: "Check for Updates…",
      description: "Check whether a newer version of Writ is available",
      scope: "app",
      execute: () => updateStore.checkForUpdate(),
    });

    try {
      await registerTransformCommands();
    } catch (error) {
      showToast("Failed to load transform commands", "error");
      console.error("registerTransformCommands failed", error);
    }

    setExecuteListener((id) => configStore.recordCommandUse(id));
    configStore.pruneCommandUsage(new Set(getAllCommands().map((c) => c.id)));

    const loadedKeybindings = configStore.config().keybindings;
    const liveKeybindings = pruneLegacyDefaultOverrides(loadedKeybindings);
    if (Object.keys(liveKeybindings).length !== Object.keys(loadedKeybindings).length) {
      configStore
        .save({ ...configStore.config(), keybindings: liveKeybindings })
        .catch(() => {});
    }
    setKeybindingOverrides(liveKeybindings);
    rebuildKeyMap();
    installKeyboardHandler();
    unlisteners.push(uninstallKeyboardHandler);

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

    const unlisten4 = await onEvent("files:dropped", (payload) => {
      void openPendingPaths(payload.paths);
    });
    unlisteners.push(unlisten4);

    const offAutosaveError = onAutosaveError((bufferId) => {
      showToast(`Autosave failed for ${bufferId}`, "error");
    });
    unlisteners.push(offAutosaveError);

    const unlistenUpdate = await updateStore.subscribe();
    unlisteners.push(unlistenUpdate);
  });

  onCleanup(() => {
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
        <ThemeEditor />
        <ShortcutEditor />
        <ContextMenu />
        <ConfirmDialog />
        <ToastContainer />
        <UpdateBanner />
      </div>
    </ErrorBoundary>
  );
}
