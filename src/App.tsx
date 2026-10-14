import { onMount, onCleanup, createEffect, Show, untrack } from "solid-js";
import TitleBar from "./components/TitleBar/TitleBar";
import WindowLights from "./components/TitleBar/WindowLights";
import EditorArea from "./components/Editor/EditorArea";
import Sidebar, { focusIsOutsideSidebar } from "./components/Sidebar/Sidebar";
import RightPanel from "./components/RightPanel/RightPanel";
import ChatPane from "./components/Chat/ChatPane";
import CommandPalette, {
  openNoteSearch,
  toggleCommandPalette,
} from "./components/CommandPalette/CommandPalette";
import SearchPalette, { toggleSearchPalette } from "./components/SearchPalette/SearchPalette";
import ThemeEditor, { openThemeEditor } from "./components/ThemeEditor/ThemeEditor";
import ShortcutEditor, { openShortcutEditor } from "./components/ShortcutEditor/ShortcutEditor";
import SettingsModal, { openSettings } from "./components/SettingsModal/SettingsModal";
import ActivityPanel, { openActivity } from "./components/Activity/ActivityPanel";
import ProgramNotice from "./components/Activity/ProgramNotice";
import NoteHistoryPanel, { openNoteVersions } from "./components/NoteHistory/NoteHistoryPanel";
import NotesMigrationReport from "./components/NotesMigrationReport/NotesMigrationReport";
import { startRenameActiveTab } from "./components/Editor/TabBar";
import { confirmAndDeleteNote, noteIsDeletable, saveCopyOfNote } from "./lib/note-actions";
import ContextMenu from "./components/ContextMenu/ContextMenu";
import LinkAmbiguityPicker from "./components/Editor/LinkAmbiguityPicker";
import LanguagePicker from "./components/Editor/LanguagePicker";
import EncodingPicker from "./components/Editor/EncodingPicker";
import IconSprite from "./components/Icon/IconSprite";
import ToastContainer from "./components/Notifications/Toast";
import ConfirmDialog from "./components/ConfirmDialog/ConfirmDialog";
import LargeFileOpen from "./components/LargeFileOpen/LargeFileOpen";
import AppFrame from "./components/AppFrame/AppFrame";
import ErrorBoundary from "./components/ErrorBoundary/ErrorBoundary";
import BootError from "./components/BootError/BootError";
import UpdateBanner from "./components/UpdateBanner/UpdateBanner";
import WindowProvider, { useWindow } from "./components/WindowProvider/WindowProvider";
import FirstRunHint from "./components/Editor/FirstRunHint";
import FirstRunSetup from "./components/FirstRun/FirstRunSetup";
import AiRewriteOverlay from "./components/AiRewrite/AiRewriteOverlay";
import PromptFillModal from "./components/PromptFill/PromptFillModal";
import ContextPackModal from "./components/ContextPack/ContextPackModal";
import { installNativeContextMenuSuppressor } from "./lib/native-context-menu";
import { resolvePlatform } from "./lib/platform";
import { DOCS_URL, reportIssueUrl } from "./lib/help-links";
import { APP_VERSION } from "./lib/app-info";
import { logFailure } from "./lib/log";
import { focusAfterSidebarChange } from "./lib/sidebar-focus";
import { firstRunStore } from "./stores/global/first-run";
import { bufferRegistry } from "./stores/global/buffer-registry";
import { renameLinksStore } from "./stores/global/rename-links";
import { openThirdPartyNoticesBuffer } from "./stores/global/notices";
import { workspaceStore } from "./stores/global/workspace";
import { inboxStore } from "./stores/global/inbox";
import { updateStore } from "./stores/global/update";
import { linkStore } from "./stores/global/link";
import { windowRegistry } from "./stores/global/window-registry";
import { findStore } from "./stores/global/find-store";
import { editorZoom } from "./stores/global/editor-zoom";
import { requestConfirm } from "./stores/global/dialogs";
import { removeFromRecent } from "./stores/global/recent-removal";
import { notesStore } from "./stores/global/notes";
import { configStore } from "./stores/global/config";
import { commandUsageStore } from "./stores/global/command-usage";
import { themeStore } from "./stores/global/theme";
import { showToast } from "./stores/global/notifications";
import { createBootStore, createBootPlan, type BootShell } from "./stores/window/boot";
import { registerTransformCommands } from "./commands/transforms";
import { registerPromptCommands } from "./commands/prompt";
import { defineAppCommands, definedAppCommandIds } from "./commands/app-commands";
import { openContentSearch } from "./commands/search";
import { toggleChat } from "./commands/chat";
import { openRecent } from "./commands/recent";
import { followAppSwitches } from "./commands/app-switches";
import { registerPreviewKeymap } from "./keymap/preview";
import { registerCommand, executeCommand, getAllCommands, setExecuteListener } from "./commands/registry";
import {
  installKeyboardHandler,
  uninstallKeyboardHandler,
  rebuildKeyMap,
  setKeybindingOverrides,
  pruneLegacyDefaultOverrides,
} from "./commands/keybindings";
import "./styles/global.css";
import "./App.css";

const MAIN_WINDOW_ID = 1;

type Unlisten = () => void;

/**
 * The window. The frame paints the window and holds the crash screen, so a
 * render error still leaves a window that can be moved and closed; the toasts
 * and the update banner sit beside it, outside every modal's focus trap and
 * outside the error boundary, so they report what happened in either.
 */
export default function App() {
  return (
    <>
      <AppFrame>
        <ErrorBoundary>
          <WindowProvider windowId={MAIN_WINDOW_ID}>
            <AppShell />
          </WindowProvider>
        </ErrorBoundary>
      </AppFrame>
      <ToastContainer />
      <UpdateBanner />
    </>
  );
}

// Follow-system polarity. Registered in onMount and removed through the
// onCleanup list below, never at module scope.
function watchSystemPolarity(): Unlisten {
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

/**
 * Takes the first launch's one line away on the first keystroke.
 *
 * A keydown rather than an edit: the line is about where the note goes, and
 * the person has read it or not by the time they touch the keyboard. A
 * modifier held on its own is not a keystroke.
 */
function dismissHintOnFirstKeystroke(): Unlisten {
  const onKeyDown = (event: KeyboardEvent) => {
    if (["Shift", "Control", "Alt", "Meta"].includes(event.key)) return;
    firstRunStore.dismissHint();
    document.removeEventListener("keydown", onKeyDown);
  };
  document.addEventListener("keydown", onKeyDown);
  return () => document.removeEventListener("keydown", onKeyDown);
}

/** The file behind the tab in front, for a tab that has one. */
function activeNotePath(): string | null {
  const id = windowRegistry.getActive()?.tabs.activeTabId();
  if (!id) return null;
  return bufferRegistry.activeTabs().find((b) => b.id === id)?.source_path ?? null;
}

/** Reads the shortcut overrides and starts answering chords. */
function installKeyboard(): Unlisten {
  const loadedKeybindings = configStore.config().keybindings;
  const liveKeybindings = pruneLegacyDefaultOverrides(loadedKeybindings);
  if (Object.keys(liveKeybindings).length !== Object.keys(loadedKeybindings).length) {
    configStore.save({ ...configStore.config(), keybindings: liveKeybindings }).catch(() => {});
  }
  setKeybindingOverrides(liveKeybindings);
  rebuildKeyMap();
  installKeyboardHandler();
  return uninstallKeyboardHandler;
}

function AppShell() {
  const win = useWindow();
  const boot = createBootStore();
  const unlisteners: Unlisten[] = [];
  let disposed = false;

  // Registered here rather than with the rest: the label names the app the
  // folder opens in, and that word comes from Rust with the first-run state,
  // so the registration follows the answer instead of the mount.
  createEffect(() => {
    registerCommand({
      id: "notes.showFolder",
      icon: "folder-open",
      label: `Show folder in ${firstRunStore.fileManager()}`,
      scope: "app",
      execute: () => {
        void notesStore.showInFileManager().catch(() => {
          showToast("Could not open the folder", "error");
        });
      },
    });
  });

  /** The window's own commands: files, tabs, panels, search, settings. */
  function registerCommands(): void {
    registerCommand({
      id: "note.new",
      icon: "note-pencil",
      label: "New file",
      description: "Create a file in your folder",
      keybinding: "CmdOrCtrl+N",
      // The chord this command answered to before it was named for the note
      // rather than the buffer.
      keybindingAliases: ["CmdOrCtrl+T"],
      scope: "app",
      global: true,
      execute: () => void windowRegistry.getActive()?.tabs.newNote(),
    });

    registerCommand({
      id: "note.today",
      label: "Today's file",
      description: "Open the file dated today, or create it",
      scope: "app",
      execute: () => void windowRegistry.getActive()?.tabs.todaysNote(),
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
      description: "Write the active file to disk now",
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
      description: "Show a folder's files in the sidebar",
      keybinding: "CmdOrCtrl+Alt+O",
      scope: "app",
      // Global: the editor holds focus almost all the time.
      global: true,
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
      description: "Stop showing this folder in the sidebar",
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
      id: "history.openRecent",
      label: "Open recent",
      description: "Show the files closed most recently",
      scope: "app",
      execute: () => openRecent(),
    });

    registerCommand({
      id: "sidebar.toggle",
      icon: "sidebar-simple",
      label: "Toggle sidebar",
      keywords: ["show", "hide"],
      keybinding: "CmdOrCtrl+\\",
      // Finder's own sidebar chord, for the hand that already knows it.
      keybindingAliases: ["CmdOrCtrl+Alt+S"],
      scope: "app",
      // Global: the editor holds focus almost all the time in a writing app, so
      // a focus-gated sidebar toggle would be unreachable from the keyboard. It
      // must fire from the editor and from the sidebar search input alike.
      global: true,
      execute: () => windowRegistry.getActive()?.sidebar.toggle(),
    });

    defineAppCommands("connections", [
      {
        id: "panel.toggle",
        icon: "link-simple",
        label: "Toggle connections",
        description: "Show or hide what links to this file, its outline and properties",
        keybinding: "CmdOrCtrl+Shift+\\",
        scope: "app",
        // Global, for the same reason the sidebar's toggle is: the editor holds
        // focus almost all the time, so a focus-gated chord would never reach it.
        global: true,
        execute: () => windowRegistry.getActive()?.rightPanel.toggle(),
      },
    ]);

    registerCommand({
      id: "chat.toggle",
      icon: "chat-text",
      label: "Toggle chat",
      description: "Ask a model about the files you attach",
      keywords: ["chat", "ai", "ask", "model"],
      keybinding: "CmdOrCtrl+Shift+A",
      scope: "app",
      // Global, for the reason the two toggles above it are: the editor holds
      // focus almost all the time.
      global: true,
      // Registered while chat is off so its chord can open the switch; its
      // app keeps it out of the palette and both menus.
      app: "chat",
      execute: () => toggleChat(),
    });

    defineAppCommands("graph", [
      {
        id: "folderGraph.open",
        icon: "folder-simple",
        label: "Open graph",
        description: "Show every file in the folder and the links between them",
        scope: "app",
        // Global, like the two toggles above: the editor holds focus while you
        // write, and this is reached from there or from nowhere.
        global: true,
        execute: () => windowRegistry.getActive()?.folderGraph.open(),
      },
    ]);

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
      // Cmd+R is Reload everywhere else on a Mac, and Cocoa has no top-level
      // Replace chord. The Find bar carries the control instead.
      keybinding: "CmdOrCtrl+Alt+F",
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
      label: "Open file by name",
      description: "Find a file by name and open it",
      keybinding: "CmdOrCtrl+Shift+O",
      scope: "app",
      global: true,
      execute: () => openNoteSearch(),
    });

    registerCommand({
      id: "note.rename",
      label: "Rename file…",
      description: "Rename the active file",
      keybinding: "F2",
      keybindingAliases: ["CmdOrCtrl+Shift+S"],
      scope: "app",
      execute: () => startRenameActiveTab(),
    });

    registerCommand({
      id: "note.versions",
      label: "Revert to…",
      description: "Read earlier versions of this file, and put one back",
      scope: "app",
      isAvailable: () => activeNotePath() !== null,
      execute: () => {
        const path = activeNotePath();
        if (path) openNoteVersions(path);
      },
    });

    registerCommand({
      id: "note.undoRename",
      label: "Undo rename",
      description: "Put the file's name back, and the links that were updated",
      scope: "app",
      isAvailable: () => renameLinksStore.canUndo(),
      execute: () => void renameLinksStore.undoRename(),
    });

    registerCommand({
      id: "note.delete",
      label: "Delete file",
      description: "Move the active file to the Trash",
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
      description: "Write a copy of the active file into your folder",
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
            "Each tab that saves moves to Recent, where you can reopen it. A tab that cannot save stays open.",
          confirmLabel: "Close all",
        });
        if (!confirmed) return;
        const front = w.tabs.activeTabId();
        const closed = await w.tabs.closeAllTabs();
        if (closed.length === 0) return;
        showToast({
          text: closed.length === 1 ? "Closed 1 tab" : `Closed ${closed.length} tabs`,
          type: "success",
          action: { label: "Undo", run: () => w.tabs.reopenTabs(closed, front) },
        });
      },
    });

    registerCommand({
      id: "history.clear",
      label: "Clear Recent",
      scope: "app",
      execute: async () => {
        const ids = bufferRegistry.historyList().map((b) => b.id);
        const count = ids.length;
        if (count === 0) return;
        const confirmed = await requestConfirm({
          title: "Clear Recent?",
          message: `This removes ${count} file${count === 1 ? "" : "s"} from Recent. The files stay where they are.`,
          confirmLabel: "Clear Recent",
          danger: true,
        });
        if (confirmed) removeFromRecent(ids, "Cleared Recent");
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
        commandUsageStore.clear().then(
          () => showToast("Command usage cleared", "success"),
          () => showToast("Failed to clear command usage", "error"),
        );
      },
    });

    registerCommand({
      id: "shortcuts.customize",
      label: "Keyboard shortcuts",
      description: "List every shortcut and change any of them",
      keywords: ["customize", "rebind", "keys", "chord"],
      scope: "app",
      execute: () => openShortcutEditor(),
    });

    registerCommand({
      id: "help.docs",
      label: "Writ help",
      description: "Open the guide on the Writ site",
      keywords: ["docs", "documentation", "manual", "guide"],
      scope: "app",
      execute: () => void linkStore.openExternal(DOCS_URL),
    });

    registerCommand({
      id: "help.whatsNew",
      label: "What's new",
      description: `Open the changelog at version ${APP_VERSION}`,
      keywords: ["changelog", "release notes", "version"],
      scope: "app",
      execute: () => void updateStore.openReleaseNotes(APP_VERSION),
    });

    registerCommand({
      id: "help.showSetup",
      label: "Show setup again",
      description: "Choose the format for new files and the apps that are on",
      keywords: ["first run", "welcome", "onboarding", "apps"],
      scope: "app",
      execute: () => firstRunStore.showSetup(),
    });

    registerCommand({
      id: "help.reportIssue",
      label: "Report an issue",
      description: "Open the bug form on GitHub, with this version filled in",
      keywords: ["bug", "feedback", "problem", "github"],
      scope: "app",
      execute: () => void linkStore.openExternal(reportIssueUrl(APP_VERSION, resolvePlatform())),
    });

    defineAppCommands("programs", [
      {
        id: "activity.open",
        icon: "plugs-connected",
        label: "Program activity",
        description: "What connected programs did with your files",
        keywords: ["mcp", "programs", "claude", "cursor", "connected"],
        scope: "app",
        execute: () => openActivity(),
      },
    ]);

    registerCommand({
      id: "settings.open",
      icon: "gear",
      label: "Settings",
      description: "Folder, apps, appearance and shortcuts",
      keybinding: "CmdOrCtrl+,",
      scope: "app",
      global: true,
      execute: () => openSettings(),
    });

    registerCommand({
      id: "app.thirdPartyNotices",
      label: "Third-party licences",
      description: "Open the licences of the code Writ is built on",
      scope: "app",
      execute: () => {
        void (async () => {
          try {
            const { doc, reused } = await openThirdPartyNoticesBuffer();
            const active = windowRegistry.getActive();
            active?.tabs.setActiveTabId(doc.id);
            // Reopening rewrites the same file, and activating a tab that is
            // already active loads nothing, so the text is pulled in here.
            if (reused) active?.editor.requestExternalReload(doc.id);
          } catch {
            showToast("Could not open the third-party licences", "error");
          }
        })();
      },
    });

    registerCommand({
      id: "app.check_updates",
      label: "Check for updates…",
      description: "Check whether a newer version of Writ is available",
      scope: "app",
      execute: () => updateStore.checkForUpdate(),
    });
  }

  const shell: BootShell = {
    prepareShell: () => {
      const off: Unlisten[] = [];
      // Writ owns every context menu; the engine's belongs to a browser.
      off.push(installNativeContextMenuSuppressor());
      // The platform layer is a token overlay keyed off the root; Writ is
      // single-window, so it is written once and never recomputed (ADR-030).
      document.documentElement.setAttribute("data-platform", resolvePlatform());
      themeStore.applyToRoot();
      off.push(watchSystemPolarity());
      off.push(dismissHintOnFirstKeystroke());
      return off;
    },
    registerCommands,
    installKeyboard,
    runMenuAction: (id) => executeCommand(id),
    afterReveal: [
      [
        "transforms",
        async () => {
          try {
            await registerTransformCommands();
          } catch {
            showToast("Failed to load transform commands", "error");
            logFailure("transform commands could not be registered");
          }
        },
      ],
      ["prompts", () => registerPromptCommands()],
      ["preview keymap", () => registerPreviewKeymap()],
      [
        "command usage",
        async () => {
          await commandUsageStore.load();
          setExecuteListener((id) => commandUsageStore.record(id));
          commandUsageStore.prune(
            new Set([...getAllCommands().map((c) => c.id), ...definedAppCommandIds()]),
          );
        },
      ],
      // The commands registered since the keyboard handler was installed.
      ["key map", () => rebuildKeyMap()],
    ],
  };

  onMount(() => {
    void boot.run(createBootPlan(win, shell)).then((off) => {
      if (disposed) for (const unlisten of off) unlisten();
      else unlisteners.push(...off);
    });
  });

  onCleanup(() => {
    disposed = true;
    for (const unlisten of unlisteners) {
      unlisten();
    }
  });

  // Toggling the sidebar changes what is visible, not where you type: showing
  // it leaves the caret in the note, hiding it returns focus to the note so
  // nothing stays focused inside a collapsed region. Focus on a control
  // outside the sidebar, the toolbar's search field clearing its query, stays.
  createEffect(() => {
    const open = win.sidebar.isOpen();
    if (focusAfterSidebarChange(open, untrack(focusIsOutsideSidebar)) === "editor") {
      win.editor.focusEditor();
    }
  });

  // Commands come and go, and panels close, with the app switches.
  followAppSwitches(win);

  return (
    <>
      <IconSprite />
      <TitleBar />
      <div class="app-body">
        <Sidebar />
        <EditorArea />
        <RightPanel />
        <ChatPane />
        <FirstRunHint />
        <Show when={boot.failure()}>
          {(failure) => <BootError failure={failure()} onRetry={boot.retry} />}
        </Show>
        {/* Last in the row and over both panes: the lights sit at the window's
            leading edge whatever the sidebar is doing under them. */}
        <WindowLights />
      </div>
      <CommandPalette />
      <SearchPalette />
      <SettingsModal />
      <ActivityPanel />
      <ProgramNotice />
      <NoteHistoryPanel />
      <ThemeEditor />
      <ShortcutEditor />
      <ContextMenu />
      <LinkAmbiguityPicker />
      <LanguagePicker />
      <EncodingPicker />
      <ConfirmDialog />
      <LargeFileOpen />
      <PromptFillModal />
      <ContextPackModal />
      <AiRewriteOverlay />
      <NotesMigrationReport />
      <Show when={firstRunStore.step() !== null}>
        <FirstRunSetup />
      </Show>
    </>
  );
}
