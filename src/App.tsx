import { onMount, onCleanup, createEffect } from "solid-js";
import TitleBar from "./components/TitleBar/TitleBar";
import WindowLights from "./components/TitleBar/WindowLights";
import EditorArea from "./components/Editor/EditorArea";
import Sidebar from "./components/Sidebar/Sidebar";
import CommandPalette, {
  openNoteSearch,
  toggleCommandPalette,
} from "./components/CommandPalette/CommandPalette";
import SearchPalette, { toggleSearchPalette } from "./components/SearchPalette/SearchPalette";
import ThemeEditor, { openThemeEditor } from "./components/ThemeEditor/ThemeEditor";
import ShortcutEditor, { openShortcutEditor } from "./components/ShortcutEditor/ShortcutEditor";
import SettingsModal, { openSettings } from "./components/SettingsModal/SettingsModal";
import NotesMigrationReport from "./components/NotesMigrationReport/NotesMigrationReport";
import { startRenameActiveTab } from "./components/Editor/TabBar";
import { confirmAndDeleteNote, noteIsDeletable, saveCopyOfNote } from "./lib/note-actions";
import ContextMenu from "./components/ContextMenu/ContextMenu";
import LinkAmbiguityPicker from "./components/Editor/LinkAmbiguityPicker";
import IconSprite from "./components/Icon/IconSprite";
import { installNativeContextMenuSuppressor } from "./lib/native-context-menu";
import { IS_MAC, resolvePlatform } from "./lib/platform";
import ToastContainer, { showToast } from "./components/Notifications/Toast";
import ConfirmDialog, { requestConfirm } from "./components/ConfirmDialog/ConfirmDialog";
import AppFrame from "./components/AppFrame/AppFrame";
import ErrorBoundary from "./components/ErrorBoundary/ErrorBoundary";
import UpdateBanner from "./components/UpdateBanner/UpdateBanner";
import WindowProvider, { useWindow } from "./components/WindowProvider/WindowProvider";
import { bufferRegistry } from "./stores/global/buffer-registry";
import { saveStatusStore } from "./stores/global/save-status";
import { basename } from "./lib/path";
import { logFailure } from "./lib/log";
import { workspaceStore } from "./stores/global/workspace";
import { notesStore } from "./stores/global/notes";
import { inboxStore } from "./stores/global/inbox";
import { updateStore } from "./stores/global/update";
import { configStore } from "./stores/global/config";
import { themeStore } from "./stores/global/theme";
import { osWindowStore } from "./stores/global/os-window";
import { windowRegistry } from "./stores/global/window-registry";
import { focusAfterSidebarChange } from "./lib/sidebar-focus";
import { openContentSearch } from "./commands/search";
import { findStore } from "./stores/global/find-store";
import { registerTransformCommands } from "./commands/transforms";
import { registerPromptCommands } from "./commands/prompt";
import { registerAiCommands, unregisterAiCommands } from "./commands/ai";
import { aiRewriteStore } from "./stores/global/ai-rewrite";
import AiRewriteOverlay from "./components/AiRewrite/AiRewriteOverlay";
import PromptFillModal from "./components/PromptFill/PromptFillModal";
import { registerPreviewKeymap } from "./keymap/preview";
import { rendererRegistry } from "./stores/global/renderer-registry";
import { probeDefaultAppSupport } from "./stores/global/default-app-support";
import { previewListRenderers, getRecoveredBuffers } from "./services/tauri";
import { editorZoom } from "./stores/global/editor-zoom";
import { registerCommand, executeCommand, getAllCommands, setExecuteListener } from "./commands/registry";
import {
  installKeyboardHandler,
  uninstallKeyboardHandler,
  rebuildKeyMap,
  setKeybindingOverrides,
  pruneLegacyDefaultOverrides,
} from "./commands/keybindings";
import { onEvent, emitFrontendReady } from "./services/events";
import { handleExternalEdit, readExternalEditPayload } from "./services/external-edit";
import { createExternalEditDeps } from "./lib/external-edit-deps";
import { recheckOpenNotes } from "./services/notes-sweep";
import { reportFirstPaint } from "./services/tauri";
import { installCloseFlush, startWindowLifecycle } from "./services/window-lifecycle";
import type { UnlistenFn } from "./services/events";
import "./styles/global.css";
import "./App.css";

const MAIN_WINDOW_ID = 1;

// A save failure names the file the user knows, never the buffer UUID.
async function openPendingPaths(paths: string[]) {
  if (!Array.isArray(paths)) {
    logFailure("an open request arrived without a list of files");
    showToast("Couldn't open the files", "error");
    return;
  }
  const win = windowRegistry.getActive();
  if (!win) {
    logFailure("an open request arrived with no window to open into");
    showToast("Couldn't open the files", "error");
    return;
  }
  // A drop of twenty unreadable files is one failure to the user, not twenty.
  const failed: string[] = [];
  for (const path of paths) {
    try {
      await win.tabs.openFile(path);
    } catch {
      failed.push(basename(path));
    }
  }
  if (failed.length > 0) {
    logFailure("a file could not be opened");
    showToast(
      failed.length === 1 ? `Couldn't open ${failed[0]}` : `Couldn't open ${failed.length} files`,
      "error",
    );
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

// Follow-system polarity. Registered in onMount and removed through the
// onCleanup list below, never at module scope.
function watchSystemPolarity(): UnlistenFn {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return () => {};
  }
  const query = window.matchMedia("(prefers-color-scheme: dark)");
  const onChange = (event: MediaQueryListEvent) => {
    themeStore.setSystemPolarity(event.matches ? "dark" : "light");
  };
  query.addEventListener("change", onChange);
  themeStore.setSystemPolarity(query.matches ? "dark" : "light");
  return () => query.removeEventListener("change", onChange);
}

function AppShell() {
  const win = useWindow();
  const unlisteners: UnlistenFn[] = [];

  onMount(async () => {
    measureFirstPaint("cold");
    // Writ owns every context menu; the engine's belongs to a browser.
    onCleanup(installNativeContextMenuSuppressor());
    // The platform layer is a token overlay keyed off the root; Writ is
    // single-window, so it is written once and never recomputed (ADR-030).
    document.documentElement.setAttribute("data-platform", resolvePlatform());
    themeStore.applyToRoot();
    await configStore.load();
    // A config written before ADR-030 carries overrides in the old vocabulary.
    // They are translated on load; writing the result back means the next
    // launch reads a clean map.
    const migratedOverrides = themeStore.loadConfig(
      configStore.config().theme,
      configStore.config().appearance,
    );
    if (migratedOverrides) {
      const current = configStore.config();
      configStore
        .save({ ...current, theme: { ...current.theme, overrides: migratedOverrides } })
        .catch(() => {});
    }
    unlisteners.push(watchSystemPolarity());
    unlisteners.push(await osWindowStore.installFocusSync());
    // Only the Windows and Linux titlebars read maximized(); on macOS this
    // would be an IPC round-trip per resize feeding a signal nothing renders.
    if (!IS_MAC) unlisteners.push(await osWindowStore.installMaximizeSync());
    unlisteners.push(await osWindowStore.installGeometryPersistence());
    unlisteners.push(await installCloseFlush([() => osWindowStore.flushGeometry()]));
    unlisteners.push(...(await startWindowLifecycle()));
    win.sidebar.hydrateFromConfig();
    await bufferRegistry.load();
    await workspaceStore.hydrate().catch(() => undefined);
    await inboxStore.hydrate().catch(() => undefined);
    await notesStore.load();

    // Only notes whose text never reached their file are restored, so the
    // count is how many of those there were, not how many tabs were open.
    //
    // A note whose file was deleted is not among them: the launch wrote
    // nothing for it (ADR-033 decision 15), so its text is seeded into the
    // store instead, before any tab loads, and its tab comes up carrying the
    // bar and the three ways out. Counting it as restored would say a file is
    // back that is not.
    const recovered = await getRecoveredBuffers().catch(() => []);
    for (const note of recovered) {
      if (note.removed_on_disk) win.editor.markRemovedOnDisk(note.id, note.content);
    }
    const restored = recovered.filter((note) => !note.removed_on_disk);
    if (restored.length > 0) {
      showToast(
        restored.length === 1
          ? "Restored 1 note that could not be saved last time"
          : `Restored ${restored.length} notes that could not be saved last time`,
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

    const unlistenInbox = await onEvent("inbox:file-arrived", (payload) => {
      void inboxStore.handleFileArrived(payload.path);
    });
    unlisteners.push(unlistenInbox);

    await emitFrontendReady();

    if (win.tabs.activeTabId() === null) {
      const active = bufferRegistry.activeTabs();
      if (active.length === 0) {
        await win.tabs.createTab();
      } else {
        win.tabs.setActiveTabId(active[active.length - 1].id);
      }
    }

    // Reapplied here rather than in the hidden Rust restore path: on Windows
    // maximizing runs ShowWindow(SW_MAXIMIZE), which has no visibility guard,
    // so it would put an unpainted frame on screen for the whole boot and turn
    // the reveal below into a no-op. By this point the webview has painted.
    if (configStore.config().window.maximized) await osWindowStore.maximize();

    // The window was created hidden to avoid a cold-start flash; reveal it now
    // that content and the active tab are in place. Showing directly (not via
    // requestAnimationFrame, which a browser may throttle for a hidden
    // document) makes the window appear promptly without waiting on the Rust
    // fallback; the webview paints the already-built DOM as it becomes visible.
    void osWindowStore.reveal();

    registerCommand({
      id: "note.new",
      icon: "note-pencil",
      label: "New note",
      description: "Create a note in the notes folder",
      keybinding: "CmdOrCtrl+N",
      // The chord this command answered to before it was named for the note
      // rather than the buffer.
      keybindingAliases: ["CmdOrCtrl+T"],
      scope: "app",
      global: true,
      execute: () => void windowRegistry.getActive()?.tabs.newNote(),
    });

    registerCommand({
      id: "file.open",
      label: "Open file",
      description: "Open a file from disk into a new tab",
      keybinding: "CmdOrCtrl+O",
      scope: "app",
      global: true,
      execute: () => windowRegistry.getActive()?.tabs.openFileDialog(),
    });

    registerCommand({
      id: "buffer.save",
      label: "Save",
      description: "Write the active note to disk now",
      keybinding: "CmdOrCtrl+S",
      scope: "app",
      // Global: the editor holds focus while writing, so a focus-gated save
      // would never fire from where it is pressed.
      global: true,
      execute: () => void windowRegistry.getActive()?.editor.saveActiveBuffer(),
    });

    registerCommand({
      id: "workspace.openFolder",
      icon: "folder-open",
      label: "Open folder…",
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
      label: "Close folder",
      description: "Close the open workspace folder",
      scope: "app",
      execute: () => void workspaceStore.closeFolder(),
    });

    registerCommand({
      id: "inbox.watchFolder",
      label: "Watch folder…",
      description: "Auto-open new files that appear in a folder",
      scope: "app",
      execute: () => void inboxStore.watchFolder(),
    });

    registerCommand({
      id: "inbox.stopWatching",
      label: "Stop watching folder",
      description: "Stop auto-opening files from the watched folder",
      scope: "app",
      execute: () => void inboxStore.stopWatching(),
    });

    registerCommand({
      id: "buffer.close",
      label: "Close tab",
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
      label: "Next tab",
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
      label: "Previous tab",
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
      label: "Reopen closed tab",
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
      icon: "sidebar-simple",
      label: "Toggle sidebar",
      description: "Show or hide the tabs + history rail",
      keybinding: "CmdOrCtrl+\\",
      scope: "app",
      // Global: the editor holds focus almost all the time in a writing app, so
      // a focus-gated sidebar toggle would be unreachable from the keyboard. It
      // must fire from the editor and from the sidebar search input alike.
      global: true,
      execute: () => windowRegistry.getActive()?.sidebar.toggle(),
    });

    registerCommand({
      id: "search.openContent",
      label: "Search text…",
      scope: "app",
      execute: openContentSearch,
    });

    registerCommand({
      id: "search.openEverywhere",
      icon: "magnifying-glass",
      label: "Search everywhere",
      description: "Search commands, settings, file names and text",
      keybinding: "CmdOrCtrl+Shift+F",
      scope: "app",
      // Global: the editor holds focus almost all the time, so a focus-gated
      // chord would never reach the handler from where it is used.
      global: true,
      execute: () => toggleSearchPalette(),
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
      label: "Find next",
      description: "Move to the next match",
      keybinding: "CmdOrCtrl+G",
      scope: "editor",
      execute: () => findStore.findNextCmd(),
    });

    registerCommand({
      id: "editor.findPrevious",
      label: "Find previous",
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
      id: "editor.zoomIn",
      label: "Increase editor font size",
      description: "Make the editor text larger",
      keybinding: "CmdOrCtrl+=",
      keybindingAliases: ["CmdOrCtrl+Shift++"],
      scope: "app",
      // Global: the editor holds focus almost always, so a focus-gated zoom
      // chord would never reach the handler from where it is used.
      global: true,
      execute: () => editorZoom.zoomIn(),
    });

    registerCommand({
      id: "editor.zoomOut",
      label: "Decrease editor font size",
      description: "Make the editor text smaller",
      keybinding: "CmdOrCtrl+-",
      scope: "app",
      global: true,
      execute: () => editorZoom.zoomOut(),
    });

    registerCommand({
      id: "editor.zoomReset",
      label: "Reset editor font size",
      description: "Restore the editor text to its default size",
      keybinding: "CmdOrCtrl+0",
      scope: "app",
      global: true,
      execute: () => editorZoom.reset(),
    });

    registerCommand({
      id: "palette.open",
      label: "Command palette",
      description: "Search and run any command",
      keybinding: "Shift+Shift",
      scope: "app",
      global: true,
      execute: () => toggleCommandPalette(),
    });

    registerCommand({
      id: "notes.quickOpen",
      label: "Open note",
      description: "Find a note by name and open it",
      keybinding: "CmdOrCtrl+Shift+O",
      scope: "app",
      global: true,
      execute: () => openNoteSearch(),
    });

    registerCommand({
      id: "note.rename",
      label: "Rename note…",
      description: "Rename the active note and its file",
      keybinding: "F2",
      keybindingAliases: ["CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => startRenameActiveTab(),
    });

    registerCommand({
      id: "note.delete",
      label: "Delete note",
      description: "Move the active note to the Trash",
      scope: "app",
      isAvailable: () => {
        const id = windowRegistry.getActive()?.tabs.activeTabId();
        return id !== null && id !== undefined && noteIsDeletable(id);
      },
      execute: () => {
        const id = windowRegistry.getActive()?.tabs.activeTabId();
        if (id) void confirmAndDeleteNote(id);
      },
    });

    registerCommand({
      id: "note.saveCopy",
      label: "Save a copy…",
      description: "Write a copy of the active note into the notes folder",
      scope: "app",
      execute: () => {
        const id = windowRegistry.getActive()?.tabs.activeTabId();
        if (id) void saveCopyOfNote(id);
      },
    });

    registerCommand({
      id: "buffer.closeAll",
      label: "Close all tabs",
      description: "Move every open tab into history",
      scope: "app",
      execute: async () => {
        const w = windowRegistry.getActive();
        if (!w) return;
        const tabs = bufferRegistry.activeTabs();
        if (tabs.length === 0) return;
        const confirmed = await requestConfirm({
          title: "Close all tabs?",
          message:
            "Each tab that saves moves to history, where you can reopen it. A tab that cannot save stays open.",
          confirmLabel: "Close all",
        });
        if (confirmed) void w.tabs.closeAllTabs();
      },
    });

    registerCommand({
      id: "history.clear",
      label: "Clear history",
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
      icon: "gear",
      label: "Settings",
      description: "Open editor settings",
      keybinding: "CmdOrCtrl+,",
      scope: "app",
      global: true,
      execute: () => openSettings(),
    });

    registerCommand({
      id: "app.check_updates",
      label: "Check for updates…",
      description: "Check whether a newer version of Writ is available",
      scope: "app",
      execute: () => updateStore.checkForUpdate(),
    });

    try {
      await registerTransformCommands();
    } catch {
      showToast("Failed to load transform commands", "error");
      logFailure("transform commands could not be registered");
    }

    registerPromptCommands();

    try {
      const list = await previewListRenderers();
      rendererRegistry.setFromIpc(list);
    } catch {
      logFailure("preview renderers could not be listed");
      showToast("Preview is unavailable. Restart Writ to try again.", "error");
    }

    registerPreviewKeymap();

    setExecuteListener((id) => configStore.recordCommandUse(id));
    configStore.pruneCommandUsage(new Set(getAllCommands().map((c) => c.id)));

    // Resolve default-app platform support up front so settings search and the
    // command palette can offer those rows before the Settings modal mounts.
    void probeDefaultAppSupport();

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

    const externalEditDeps = createExternalEditDeps({
      editor: win.editor,
      openBuffers: () => bufferRegistry.buffers(),
      refreshBuffer: (id) => bufferRegistry.refreshBuffer(id),
      forgetSaveStatus: (id) => saveStatusStore.forgetNote(id),
    });

    const unlisten2 = await onEvent("buffer:external", (payload) => {
      const change = readExternalEditPayload(payload);
      if (!change) return;
      void handleExternalEdit(change, externalEditDeps);
    });
    unlisteners.push(unlisten2);

    // The notes folder changed faster than the watcher could list it, so no
    // file was named and every open note asks after its own.
    const unlistenSwept = await onEvent("notes:swept", () => {
      void recheckOpenNotes({
        openNotes: () => bufferRegistry.buffers(),
        diskStateOf: (id) => win.editor.readDiskState(id),
        lastKnownDiskHash: (id) => win.editor.lastKnownDiskHash(id),
        onChanged: (payload) => handleExternalEdit(payload, externalEditDeps),
      });
    });
    unlisteners.push(unlistenSwept);

    const unlisten3 = await onEvent("menu:action", (payload) => {
      executeCommand(payload.action);
    });
    unlisteners.push(unlisten3);

    const unlisten4 = await onEvent("files:dropped", (payload) => {
      void openPendingPaths(payload.paths);
    });
    unlisteners.push(unlisten4);

    const unlistenUpdate = await updateStore.subscribe();
    unlisteners.push(unlistenUpdate);

    const unlistenAi = await onEvent("ai:rewrite", (payload) => {
      aiRewriteStore.handleStreamEvent(payload);
    });
    unlisteners.push(unlistenAi);
  });

  onCleanup(() => {
    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  // Toggling the sidebar changes what is visible, not where you type: showing
  // it leaves the caret in the note, hiding it returns focus to the note so
  // nothing stays focused inside a collapsed region.
  createEffect(() => {
    if (focusAfterSidebarChange(win.sidebar.isOpen()) === "editor") win.editor.focusEditor();
  });

  // Rewrite commands exist in the palette only while the feature is on.
  createEffect(() => {
    if (configStore.config().ai.enabled) registerAiCommands();
    else unregisterAiCommands();
  });

  return (
    <AppFrame>
      <IconSprite />
      <TitleBar />
      <div class="app-body">
        <Sidebar />
        <EditorArea />
        {/* Last in the row and over both panes: the lights sit at the window's
            leading edge whatever the sidebar is doing under them. */}
        <WindowLights />
      </div>
      <CommandPalette />
      <SearchPalette />
      <SettingsModal />
      <ThemeEditor />
      <ShortcutEditor />
      <ContextMenu />
      <LinkAmbiguityPicker />
      <ConfirmDialog />
      <PromptFillModal />
      <AiRewriteOverlay />
      <ToastContainer />
      <UpdateBanner />
      <NotesMigrationReport />
    </AppFrame>
  );
}
