import { createSignal } from "solid-js";
import * as api from "../../services/tauri";
import { onEvent, emitFrontendReady, type UnlistenFn } from "../../services/events";
import { handleExternalEdit, readExternalEditPayload } from "../../services/external-edit";
import { recheckOpenNotes } from "../../services/notes-sweep";
import { installCloseFlush, startWindowLifecycle } from "../../services/window-lifecycle";
import { armReveal, type ArmedReveal } from "../../lib/boot-reveal";
import { createExternalEditDeps } from "../../lib/external-edit-deps";
import { logFailure } from "../../lib/log";
import { openFailureReason } from "../../lib/open-failure";
import { IS_MAC } from "../../lib/platform";
import { activityStore } from "../global/activity";
import { aiRewriteStore } from "../global/ai-rewrite";
import { bufferRegistry, type OpenOutcome } from "../global/buffer-registry";
import { chatStore } from "../global/chat";
import { configStore } from "../global/config";
import { syncConfigFromDisk } from "../global/config-sync";
import { probeDefaultAppSupport } from "../global/default-app-support";
import { firstRunStore, watchSavesForRetitle } from "../global/first-run";
import { hotkeyStore } from "../global/hotkey";
import { inboxStore } from "../global/inbox";
import { showToast } from "../global/notifications";
import { notesStore } from "../global/notes";
import { reportOpenOutcomes } from "../global/open-report";
import { osWindowStore } from "../global/os-window";
import { previewScriptsStore } from "../global/preview-scripts";
import { loadPreviewUrlPrefix } from "../global/preview-url";
import { rendererRegistry } from "../global/renderer-registry";
import { saveStatusStore } from "../global/save-status";
import { themeStore } from "../global/theme";
import { updateStore } from "../global/update";
import { windowRegistry } from "../global/window-registry";
import { workspaceStore } from "../global/workspace";
import { openBootTab } from "./boot-tab";
import type { WindowState } from "./createWindowState";
import { applyRecoveredNotes } from "./recovered-notes";

/** A step of the boot, named for what it does. */
export type BootStepName =
  | "shell"
  | "settings"
  | "window"
  | "session"
  | "folders"
  | "recovered"
  | "commands"
  | "keyboard"
  | "listeners"
  | "ready"
  | "first tab"
  | "after reveal";

/** Why the window shows the boot error screen instead of the editor. */
export interface BootFailure {
  step: BootStepName;
  /** The error as the step reported it, for the copied details. */
  detail: string;
}

/**
 * What the boot does, step by step. Every step but `loadSession` can fail on
 * its own: it is logged, it says so where it has a surface, and the boot goes
 * on. Without the session the window has no tabs and nothing to save into, so
 * that failure stops the boot and shows the boot error screen.
 */
export interface BootPlan {
  /** Platform marker, theme root, DOM listeners. Returns what undoes them. */
  prepareShell(): UnlistenFn[];
  loadSettings(): Promise<void>;
  watchWindow(): Promise<UnlistenFn[]>;
  loadSession(): Promise<void>;
  loadFolders(): Promise<void>;
  restoreRecovered(): Promise<void>;
  registerCommands(): void;
  installKeyboard(): UnlistenFn;
  /** Every event the window answers to, each subscribed on its own. */
  subscriptions: ReadonlyArray<readonly [string, () => Promise<UnlistenFn> | UnlistenFn]>;
  announceReady(): Promise<void>;
  armReveal(): ArmedReveal;
  showFirstTab(): Promise<void>;
  /** Work the first frame does not wait on. */
  afterReveal: ReadonlyArray<readonly [string, () => Promise<unknown> | unknown]>;
}

function describeError(error: unknown): string {
  if (error instanceof Error) return error.message;
  return String(error);
}

export function createBootStore() {
  const [failure, setFailure] = createSignal<BootFailure | null>(null);

  async function attempt<T>(
    step: string,
    work: () => T | Promise<T>,
  ): Promise<T | undefined> {
    try {
      return await work();
    } catch {
      logFailure(`the "${step}" step of the boot failed`);
      return undefined;
    }
  }

  /**
   * Runs the plan in order and returns what undoes its subscriptions.
   *
   * Every listener is attached and the keyboard handler installed before the
   * frontend tells Rust it is ready and before the window is revealed: a file
   * dropped, a menu item chosen or a chord pressed in the first second after
   * the window shows is handled, not lost.
   */
  async function run(plan: BootPlan): Promise<UnlistenFn[]> {
    const unlisteners: UnlistenFn[] = [];
    unlisteners.push(...((await attempt("shell", plan.prepareShell)) ?? []));
    await attempt("settings", plan.loadSettings);
    unlisteners.push(...((await attempt("window", plan.watchWindow)) ?? []));

    try {
      await plan.loadSession();
    } catch (error) {
      logFailure("the open tabs could not be read at startup");
      setFailure({ step: "session", detail: describeError(error) });
      plan.armReveal().now();
      return unlisteners;
    }

    await attempt("folders", plan.loadFolders);
    await attempt("recovered", plan.restoreRecovered);
    await attempt("commands", plan.registerCommands);
    const keyboard = await attempt("keyboard", plan.installKeyboard);
    if (keyboard) unlisteners.push(keyboard);

    let unheard = 0;
    for (const [kind, subscribe] of plan.subscriptions) {
      try {
        unlisteners.push(await subscribe());
      } catch {
        unheard += 1;
        logFailure(`the ${kind} event could not be subscribed`);
      }
    }
    if (unheard > 0) {
      showToast("Some file and menu events aren't reaching Writ. Restart Writ to reconnect them.", "error");
    }

    await attempt("ready", plan.announceReady);

    const reveal = plan.armReveal();
    try {
      await plan.showFirstTab();
    } catch {
      logFailure("the first tab could not be prepared");
    } finally {
      reveal.now();
    }

    for (const [name, task] of plan.afterReveal) await attempt(name, task);
    return unlisteners;
  }

  /** Loads the window again, which runs the boot from the start. */
  function retry(): void {
    window.location.reload();
  }

  return { failure, run, retry };
}

export type BootStore = ReturnType<typeof createBootStore>;

/**
 * How long the boot waits on the first-launch read before shaping the window
 * without it.
 *
 * The read decides whether a tab may be created, so it comes first; a read that
 * never settles would otherwise leave a revealed window with no editor in it.
 * Past the deadline the boot carries on as a launch with nothing to ask, which
 * is what every launch after the first one is.
 */
const FIRST_RUN_READ_DEADLINE_MS = 1500;

/** [`firstRunStore.load`], given at most `deadlineMs` to settle. */
async function readFirstRunWithin(deadlineMs: number): Promise<void> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const deadline = new Promise<void>((resolve) => {
    timer = setTimeout(resolve, deadlineMs);
  });
  try {
    await Promise.race([firstRunStore.load(), deadline]);
  } finally {
    clearTimeout(timer);
  }
}

/** Opens files the OS or a drop handed to the window, one line for every refusal. */
async function openPendingPaths(paths: string[]): Promise<void> {
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
  const outcomes: OpenOutcome[] = [];
  for (const path of paths) {
    try {
      outcomes.push(await win.tabs.openPath(path));
    } catch (error) {
      outcomes.push({ kind: "refused", path, reason: openFailureReason(error) });
    }
  }
  reportOpenOutcomes(outcomes);
}

/** Reports the time to the next frame to Rust's startup log. */
export function measureFirstPaint(
  mode: "cold" | "warm",
  rustElapsedUs: number | null = null,
): void {
  const start = performance.now();
  requestAnimationFrame(() => {
    const elapsed = performance.now() - start;
    void api.reportFirstPaint(elapsed, mode, rustElapsedUs).catch(() => undefined);
  });
}

/** What the component that boots the window does itself. */
export interface BootShell {
  /** DOM work and listeners. Returns what undoes them. */
  prepareShell(): UnlistenFn[];
  registerCommands(): void;
  installKeyboard(): UnlistenFn;
  runMenuAction(id: string): void;
  /** Work the component does once the window is on screen. */
  afterReveal: ReadonlyArray<readonly [string, () => Promise<unknown> | unknown]>;
}

/** The window's boot, wired to the stores and services it runs against. */
export function createBootPlan(win: WindowState, shell: BootShell): BootPlan {
  const externalEditDeps = createExternalEditDeps({
    editor: win.editor,
    openBuffers: () => bufferRegistry.buffers(),
    refreshBuffer: (id) => bufferRegistry.refreshBuffer(id),
    forgetSaveStatus: (id) => saveStatusStore.forgetNote(id),
    refreshScripts: (id) => previewScriptsStore.refresh(id),
  });

  return {
    prepareShell: () => {
      measureFirstPaint("cold");
      return shell.prepareShell();
    },

    loadSettings: async () => {
      // The preview URL base is host-decided; load it before any preview renders
      // so the iframe never requests a scheme the engine treats as external.
      await loadPreviewUrlPrefix(api.previewUrlPrefix);
      await configStore.load();
      void configStore.noticeUnreadValues();
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
    },

    watchWindow: async () => {
      const off: UnlistenFn[] = [];
      off.push(await osWindowStore.installFocusSync());
      // Only the Windows and Linux titlebars read maximized(); on macOS this
      // would be an IPC round-trip per resize feeding a signal nothing renders.
      if (!IS_MAC) off.push(await osWindowStore.installMaximizeSync());
      off.push(await osWindowStore.installGeometryPersistence());
      off.push(await installCloseFlush([() => osWindowStore.flushGeometry()]));
      off.push(...(await startWindowLifecycle()));
      return off;
    },

    loadSession: () => bufferRegistry.loadSession(),

    loadFolders: async () => {
      await workspaceStore.hydrate().catch(() => undefined);
      await inboxStore.hydrate().catch(() => undefined);
      await notesStore.load();
      void activityStore.watchWaiting();
    },

    restoreRecovered: async () => {
      const recovered = await api.getRecoveredBuffers().catch(() => []);
      const restored = applyRecoveredNotes(win.editor, recovered);
      if (restored.length > 0) {
        showToast(
          restored.length === 1
            ? "Restored 1 file that could not be saved last time"
            : `Restored ${restored.length} files that could not be saved last time`,
          "info",
          6000,
        );
      }
    },

    registerCommands: () => shell.registerCommands(),
    installKeyboard: () => shell.installKeyboard(),

    subscriptions: [
      ["pending:opens", () => onEvent("pending:opens", (p) => void openPendingPaths(p.paths))],
      ["files:dropped", () => onEvent("files:dropped", (p) => void openPendingPaths(p.paths))],
      [
        "window:shown",
        () => onEvent("window:shown", (p) => measureFirstPaint("warm", p.rust_elapsed_us)),
      ],
      [
        "workspace:changed",
        () => onEvent("workspace:changed", (p) => workspaceStore.handleChanged(p.path, p.removed)),
      ],
      ["workspace:swept", () => onEvent("workspace:swept", () => workspaceStore.handleSwept())],
      [
        "inbox:file-arrived",
        () => onEvent("inbox:file-arrived", (p) => void inboxStore.handleFileArrived(p.path)),
      ],
      ["config:changed", () => onEvent("config:changed", () => void syncConfigFromDisk())],
      [
        "buffer:external",
        () =>
          onEvent("buffer:external", (payload) => {
            const change = readExternalEditPayload(payload);
            if (!change) return;
            void handleExternalEdit(change, externalEditDeps);
          }),
      ],
      [
        // The notes folder changed faster than the watcher could list it, so no
        // file was named and every open note asks after its own.
        "notes:swept",
        () =>
          onEvent("notes:swept", () => {
            void recheckOpenNotes({
              tabs: bufferRegistry,
              diskStateOf: (id) => win.editor.readDiskState(id),
              lastKnownDiskHash: (id) => win.editor.lastKnownDiskHash(id),
              onChanged: (payload) => handleExternalEdit(payload, externalEditDeps),
              onFailure: () =>
                logFailure("an open note could not be checked after the folder changed"),
            });
          }),
      ],
      ["menu:action", () => onEvent("menu:action", (p) => shell.runMenuAction(p.action))],
      ["update:status", () => updateStore.subscribe()],
      // The window's own chord: what the OS said at startup, then every rebind,
      // so the shortcut editor can show a chord another app holds rather than a
      // key that quietly does nothing.
      ["hotkey:status", () => hotkeyStore.subscribe()],
      ["ai:rewrite", () => onEvent("ai:rewrite", (p) => aiRewriteStore.handleStreamEvent(p))],
      ["ai:chat", () => onEvent("ai:chat", (p) => chatStore.handleStreamEvent(p))],
      // What a new note's first line may do to its file name.
      ["saves", () => watchSavesForRetitle()],
    ],

    announceReady: () => emitFrontendReady(),

    // The window was created hidden to avoid a cold-start flash, so it takes a
    // reveal to appear. Armed before the first tab is shaped rather than
    // sequenced after it: that step can reject or hang on a slow disk, and
    // either used to end with an app running and no window on screen.
    armReveal: () => armReveal(osWindowStore.reveal),

    showFirstTab: async () => {
      // Read before the first frame is shaped, because a first launch asks its
      // question on the window the reveal is about to show rather than over a
      // note it has already put there. Under a deadline, so a read that never
      // settles costs that and nothing more.
      await readFirstRunWithin(FIRST_RUN_READ_DEADLINE_MS);
      await openBootTab(win.tabs, bufferRegistry.activeTabs(), firstRunStore.step());
      // Reapplied here rather than in the hidden Rust restore path: on Windows
      // maximizing runs ShowWindow(SW_MAXIMIZE), which has no visibility guard,
      // so it would put an unpainted frame on screen for the whole boot and turn
      // the reveal into a no-op. By this point the webview has painted.
      if (configStore.config().window.maximized) await osWindowStore.maximize();
    },

    afterReveal: [
      [
        "preview renderers",
        async () => {
          try {
            rendererRegistry.setFromIpc(await api.previewListRenderers());
          } catch {
            logFailure("preview renderers could not be listed");
            showToast("Preview is unavailable. Restart Writ to try again.", "error");
          }
        },
      ],
      ...shell.afterReveal,
      // Default-app platform support up front, so settings search and the
      // command palette can offer those rows before the Settings modal mounts.
      ["default app", () => void probeDefaultAppSupport()],
      ["hotkey", () => void hotkeyStore.load()],
    ],
  };
}
