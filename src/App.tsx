import { onMount, onCleanup, createEffect } from "solid-js";
import TitleBar from "./components/TitleBar/TitleBar";
import EditorArea from "./components/Editor/EditorArea";
import Sidebar from "./components/Sidebar/Sidebar";
import CommandPalette, { toggleCommandPalette } from "./components/CommandPalette/CommandPalette";
import ThemeEditor, { openThemeEditor } from "./components/ThemeEditor/ThemeEditor";
import ShortcutEditor, { openShortcutEditor } from "./components/ShortcutEditor/ShortcutEditor";
import SettingsModal, { openSettings } from "./components/SettingsModal/SettingsModal";
import { startRenameActiveTab } from "./components/Editor/TabBar";
import ContextMenu from "./components/ContextMenu/ContextMenu";
import ToastContainer, { showToast } from "./components/Notifications/Toast";
import ConfirmDialog, { requestConfirm } from "./components/ConfirmDialog/ConfirmDialog";
import ErrorBoundary from "./components/ErrorBoundary/ErrorBoundary";
import UpdateBanner from "./components/UpdateBanner/UpdateBanner";
import WindowProvider, { useWindow } from "./components/WindowProvider/WindowProvider";
import { bufferRegistry } from "./stores/global/buffer-registry";
import { workspaceStore } from "./stores/global/workspace";
import { updateStore } from "./stores/global/update";
import { configStore } from "./stores/global/config";
import { themeStore } from "./stores/global/theme";
import { osWindowStore } from "./stores/global/os-window";
import { windowRegistry } from "./stores/global/window-registry";
import { focusSearchBar } from "./components/Sidebar/SearchBar";
import { openContentSearch } from "./commands/search";
import { findStore } from "./stores/global/find-store";
import { registerTransformCommands } from "./commands/transforms";
import { registerPromptCommands } from "./commands/prompt";
import PromptFillModal from "./components/PromptFill/PromptFillModal";
import { registerPreviewKeymap } from "./keymap/preview";
import { rendererRegistry } from "./stores/global/renderer-registry";
import { previewListRenderers, getRecoveredBuffers } from "./services/tauri";
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
import { installCloseFlush } from "./services/window-lifecycle";
import type { UnlistenFn } from "./services/events";
import "./styles/global.css";
import "./App.css";

const MAIN_WINDOW_ID = 1;

async function openPendingPaths(paths: string[]) {
  if (!Array.isArray(paths)) {
    console.error("openPendingPaths: expected string[], got", paths);
    return;
  }
  const win = windowRegistry.getActive();
  if (!win) {
    console.error("openPendingPaths: no active window");
    return;
  }
  for (const path of paths) {
    try {
      await win.tabs.openFile(path);
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
  return (
    <ErrorBoundary>
      <WindowProvider windowId={MAIN_WINDOW_ID}>
        <AppShell />
      </WindowProvider>
    </ErrorBoundary>
  );
}

function AppShell() {
  const win = useWindow();
  const unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    measureFirstPaint("cold");
    themeStore.applyToRoot();
    await configStore.load();
    themeStore.loadConfig(configStore.config().theme);
    await osWindowStore.restoreSize();
    unlisteners.push(await osWindowStore.installFocusSync());
    unlisteners.push(await osWindowStore.installGeometryPersistence());
    unlisteners.push(await installCloseFlush([() => osWindowStore.flushGeometry()]));
    win.sidebar.hydrateFromConfig();
    await bufferRegistry.load();
    await workspaceStore.hydrate().catch(() => undefined);

    const recoveredBuffers = await getRecoveredBuffers().catch(() => []);
    if (recoveredBuffers.length > 0) {
      showToast(
        `${recoveredBuffers.length} buffer${recoveredBuffers.length === 1 ? "" : "s"} recovered from last session`,
        "info",
        6000,
      );
    }

    const unlistenPending = await onEvent("pending:opens", (payload) => {
      void openPendingPaths(payload.paths);
    });
    unlisteners.push(unlistenPending);

    const unlistenShown = await onEvent("window:shown", (payload) => {
      measureFirstPaint("warm", payload.rust_elapsed_us);
    });
    unlisteners.push(unlistenShown);

    const unlistenWorkspace = await onEvent("workspace:changed", (payload) => {
      workspaceStore.handleChanged(payload.path, payload.removed);
    });
    unlisteners.push(unlistenWorkspace);

    await emitFrontendReady();

    if (win.tabs.activeTabId() === null) {
      const active = bufferRegistry.activeTabs();
      if (active.length === 0) {
        await win.tabs.createTab();
      } else {
        win.tabs.setActiveTabId(active[active.length - 1].id);
      }
    }

    registerCommand({
      id: "buffer.new",
      label: "New Tab",
      description: "Create a new empty buffer",
      keybinding: "CmdOrCtrl+T",
      scope: "app",
      global: true,
      execute: () => windowRegistry.getActive()?.tabs.createTab(),
    });

    registerCommand({
      id: "file.open",
      label: "Open File",
      description: "Open a file from disk into a new tab",
      keybinding: "CmdOrCtrl+O",
      scope: "app",
      global: true,
      execute: () => windowRegistry.getActive()?.tabs.openFileDialog(),
    });

    registerCommand({
      id: "workspace.openFolder",
      label: "Open Folder…",
      description: "Open a folder as the workspace",
      scope: "app",
      execute: () => {
        void workspaceStore.openFolder().then((root) => {
          if (root) {
            const w = windowRegistry.getActive();
            if (w && !w.sidebar.isOpen()) w.sidebar.toggle();
          }
        });
      },
    });

    registerCommand({
      id: "workspace.closeFolder",
      label: "Close Folder",
      description: "Close the open workspace folder",
      scope: "app",
      execute: () => void workspaceStore.closeFolder(),
    });

    registerCommand({
      id: "buffer.close",
      label: "Close Tab",
      description: "Close the active tab",
      keybinding: "CmdOrCtrl+W",
      scope: "app",
      global: true,
      execute: () => {
        const w = windowRegistry.getActive();
        const id = w?.tabs.activeTabId();
        if (w && id) void w.tabs.closeTab(id);
      },
    });

    registerCommand({
      id: "buffer.nextTab",
      label: "Next Tab",
      description: "Cycle to the next open tab",
      keybinding: "CmdOrCtrl+]",
      scope: "app",
      global: true,
      execute: () => {
        const w = windowRegistry.getActive();
        if (!w) return;
        const tabs = bufferRegistry.activeTabs();
        const currentId = w.tabs.activeTabId();
        if (tabs.length < 2 || !currentId) return;
        const idx = tabs.findIndex((t) => t.id === currentId);
        const nextIdx = (idx + 1) % tabs.length;
        w.tabs.setActiveTabId(tabs[nextIdx].id);
      },
    });

    registerCommand({
      id: "buffer.prevTab",
      label: "Previous Tab",
      description: "Cycle to the previous open tab",
      keybinding: "CmdOrCtrl+[",
      scope: "app",
      global: true,
      execute: () => {
        const w = windowRegistry.getActive();
        if (!w) return;
        const tabs = bufferRegistry.activeTabs();
        const currentId = w.tabs.activeTabId();
        if (tabs.length < 2 || !currentId) return;
        const idx = tabs.findIndex((t) => t.id === currentId);
        const prevIdx = (idx - 1 + tabs.length) % tabs.length;
        w.tabs.setActiveTabId(tabs[prevIdx].id);
      },
    });

    registerCommand({
      id: "history.restoreLast",
      label: "Reopen Closed Tab",
      description: "Restore the most recently closed tab",
      keybinding: "CmdOrCtrl+Shift+T",
      scope: "app",
      global: true,
      execute: () => {
        const w = windowRegistry.getActive();
        if (!w) return;
        const history = bufferRegistry.historyList();
        if (history.length > 0) void w.tabs.restoreFromHistory(history[0].id);
      },
    });

    registerCommand({
      id: "sidebar.toggle",
      label: "Toggle Sidebar",
      description: "Show or hide the tabs + history rail",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      // Global: the editor holds focus almost all the time in a writing app, so
      // a focus-gated sidebar toggle would be unreachable from the keyboard. It
      // must fire from the editor and from the sidebar search input alike.
      global: true,
      execute: () => windowRegistry.getActive()?.sidebar.toggle(),
    });

    registerCommand({
      id: "search.openContent",
      label: "Search content…",
      scope: "app",
      execute: openContentSearch,
    });

    registerCommand({
      id: "editor.find",
      label: "Find",
      description: "Find text in the current document",
      keybinding: "CmdOrCtrl+F",
      scope: "editor",
      execute: () => findStore.open(),
    });

    registerCommand({
      id: "editor.findNext",
      label: "Find Next",
      description: "Move to the next match",
      keybinding: "CmdOrCtrl+G",
      scope: "editor",
      execute: () => findStore.findNextCmd(),
    });

    registerCommand({
      id: "editor.findPrevious",
      label: "Find Previous",
      description: "Move to the previous match",
      keybinding: "CmdOrCtrl+Shift+G",
      scope: "editor",
      execute: () => findStore.findPrevCmd(),
    });

    registerCommand({
      id: "editor.replace",
      label: "Replace",
      description: "Find and replace text in the current document",
      keybinding: "CmdOrCtrl+R",
      keybindingAliases: ["CmdOrCtrl+Alt+F"],
      scope: "editor",
      execute: () => findStore.showReplace(),
    });

    registerCommand({
      id: "palette.open",
      label: "Command Palette",
      description: "Search and run any command",
      keybinding: "Shift+Shift",
      scope: "app",
      global: true,
      execute: () => toggleCommandPalette(),
    });

    registerCommand({
      id: "tab.rename",
      label: "Rename Tab",
      description: "Rename the active tab",
      keybinding: "F2",
      keybindingAliases: ["CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => startRenameActiveTab(),
    });

    registerCommand({
      id: "buffer.closeAll",
      label: "Close All Tabs",
      description: "Move every open tab into history",
      scope: "app",
      execute: async () => {
        const w = windowRegistry.getActive();
        if (!w) return;
        const tabs = bufferRegistry.activeTabs();
        if (tabs.length === 0) return;
        const confirmed = await requestConfirm({
          title: "Close all tabs?",
          message: `All ${tabs.length} open tab${tabs.length === 1 ? "" : "s"} will move to history. You can reopen them from the sidebar.`,
          confirmLabel: "Close all",
        });
        if (confirmed) void w.tabs.closeAllTabs();
      },
    });

    registerCommand({
      id: "history.clear",
      label: "Clear History",
      description: "Permanently remove all history entries",
      scope: "app",
      execute: async () => {
        const count = bufferRegistry.historyList().length;
        if (count === 0) return;
        const confirmed = await requestConfirm({
          title: "Clear all history?",
          message: `This permanently removes ${count} closed tab${count === 1 ? "" : "s"} from history. This cannot be undone.`,
          confirmLabel: "Clear history",
          danger: true,
        });
        if (confirmed) void bufferRegistry.clearAllHistory();
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
      id: "settings.open",
      label: "Settings",
      description: "Open editor settings",
      keybinding: "CmdOrCtrl+,",
      scope: "app",
      global: true,
      execute: () => openSettings(),
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

    registerPromptCommands();

    try {
      const list = await previewListRenderers();
      rendererRegistry.setFromIpc(list);
    } catch (error) {
      console.error("previewListRenderers failed", error);
    }

    registerPreviewKeymap();

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
      bufferRegistry.load();
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
    if (win.sidebar.isOpen()) {
      focusSearchBar();
    } else {
      win.editor.focusEditor();
    }
  });

  return (
    <div class="app-container">
      <TitleBar />
      <div class="app-body">
        <Sidebar />
        <EditorArea />
      </div>
      <CommandPalette />
      <SettingsModal />
      <ThemeEditor />
      <ShortcutEditor />
      <ContextMenu />
      <ConfirmDialog />
      <PromptFillModal />
      <ToastContainer />
      <UpdateBanner />
    </div>
  );
}
