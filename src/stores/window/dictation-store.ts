import { createEffect, createSignal, onCleanup } from "solid-js";
import type { EditorView } from "@codemirror/view";
import {
  dictationCancel,
  dictationOpenMicSettings,
  dictationStart,
  dictationStop,
  type DictationEvent,
  type DictationFailure,
} from "../../services/tauri";
import { isDictationFailure, type DictationErrorCode } from "../../types/dictation";
import {
  endDictationAnchor,
  insertDictationSegment,
  showDictationPartial,
  startDictationAnchor,
} from "../../editor/dictation";
import type { PaneId } from "../../lib/pane-layout";
import { IS_MAC } from "../../lib/platform";
import { logFailure } from "../../lib/log";
import { showToast, type ToastAction } from "../global/notifications";

/** Where a session is: waiting on the start, getting the on-device model,
 * listening, or draining what was heard after Stop. */
export type DictationPhase = "idle" | "starting" | "preparing" | "recording" | "stopping";

/** What the store reads off the editor of the pane a session writes into. */
interface DictationPane {
  getView(): EditorView | null;
  currentBufferId(): string | null;
}

export interface DictationStoreDeps {
  editor: { forPane(pane: PaneId): DictationPane };
  panes: { focused(): PaneId };
}

export const NO_FILE_TO_DICTATE_INTO = "Open a file to dictate into.";
export const FILE_IS_READ_ONLY = "This file is read-only.";
export const STOPPED_ON_TAB_CHANGE = "Dictation stopped when the tab changed.";
export const START_FAILED = "Could not start dictation.";

/** The Settings row a failure's "Open Dictation settings" lands on. */
export function settingForFailure(code: DictationErrorCode): string {
  switch (code) {
    case "key_required":
    case "key_shared":
      return "dictation.api_key";
    case "model_required":
    case "model_not_found":
      return "dictation.model";
    case "unsupported_os":
      return "dictation.engine";
    case "language_unsupported":
      return "dictation.language";
    default:
      return "dictation.provider";
  }
}

/** One session, from the press that started it to its last event. */
interface Run {
  pane: DictationPane;
  view: EditorView;
  bufferId: string | null;
  sessionId: number | null;
  /** Stop or cancel came before the start answered. */
  stopRequested: boolean;
  cancelled: boolean;
}

export function createDictationStore(deps: DictationStoreDeps) {
  const [run, setRun] = createSignal<Run | null>(null);
  const [phase, setPhase] = createSignal<DictationPhase>("idle");
  const [level, setLevel] = createSignal(0);
  const [progress, setProgress] = createSignal<number | null>(null);
  const [pending, setPending] = createSignal(0);
  const [startedAt, setStartedAt] = createSignal<number | null>(null);
  const [now, setNow] = createSignal(Date.now());
  let openSettingsAt: ((settingId: string) => void) | null = null;

  function finish(current: Run): void {
    if (run() !== current) return;
    endDictationAnchor(current.view);
    setRun(null);
    setPhase("idle");
    setLevel(0);
    setProgress(null);
    setPending(0);
    setStartedAt(null);
  }

  function actionFor(failure: DictationFailure): ToastAction | undefined {
    switch (failure.action) {
      case "open_mic_settings":
        return {
          label: IS_MAC ? "Open System Settings" : "Open Settings",
          run: () =>
            dictationOpenMicSettings().catch(() => logFailure("dictation: open mic settings failed")),
        };
      case "open_dictation_settings": {
        const open = openSettingsAt;
        if (!open) return undefined;
        const settingId = settingForFailure(failure.code);
        return { label: "Open Dictation settings", run: () => open(settingId) };
      }
      default:
        return undefined;
    }
  }

  function showFailure(failure: DictationFailure): void {
    const action = actionFor(failure);
    showToast({
      text: failure.message,
      type: failure.code === "no_speech" ? "info" : "error",
      ...(action ? { action } : {}),
    });
  }

  function onEvent(current: Run, event: DictationEvent): void {
    if (run() !== current) return;
    switch (event.kind) {
      case "preparing":
        if (phase() !== "stopping") setPhase("preparing");
        setProgress(event.progress);
        return;
      case "recording":
        if (phase() !== "stopping") setPhase("recording");
        setProgress(null);
        if (startedAt() === null) {
          const at = Date.now();
          setNow(at);
          setStartedAt(at);
        }
        return;
      case "level":
        setLevel(Math.min(1, Math.max(0, event.level)));
        return;
      case "partial":
        showDictationPartial(current.view, event.text);
        return;
      case "transcribing":
        setPending(event.pending);
        return;
      case "segment":
        if (insertDictationSegment(current.view, event.text) === "anchor-lost") {
          abandon(current);
          showToast(STOPPED_ON_TAB_CHANGE, "info");
        }
        return;
      case "notice":
        showToast(event.message, "info");
        return;
      case "done":
      case "cancelled":
        finish(current);
        return;
      case "error":
        finish(current);
        showFailure(event.failure);
        return;
    }
  }

  function sendCancel(sessionId: number): void {
    dictationCancel(sessionId).catch(() => logFailure("dictation: cancel failed"));
  }

  /** Discards what is not inserted yet; inserted segments stay. */
  function abandon(current: Run): void {
    current.cancelled = true;
    if (current.sessionId !== null) sendCancel(current.sessionId);
    finish(current);
  }

  async function start(): Promise<void> {
    if (run() !== null) return;
    const pane = deps.editor.forPane(deps.panes.focused());
    const view = pane.getView();
    if (!view) {
      showToast(NO_FILE_TO_DICTATE_INTO, "info");
      return;
    }
    if (!startDictationAnchor(view)) {
      showToast(FILE_IS_READ_ONLY, "info");
      return;
    }
    const current: Run = {
      pane,
      view,
      bufferId: pane.currentBufferId(),
      sessionId: null,
      stopRequested: false,
      cancelled: false,
    };
    setRun(current);
    setPhase("starting");
    try {
      const session = await dictationStart((event) => onEvent(current, event));
      current.sessionId = session.session_id;
      if (current.cancelled) {
        sendCancel(session.session_id);
      } else if (current.stopRequested) {
        await sendStop(current);
      }
    } catch (err) {
      if (current.cancelled) return;
      finish(current);
      if (isDictationFailure(err)) {
        showFailure(err);
      } else {
        logFailure("dictation: start failed");
        showToast(START_FAILED, "error");
      }
    }
  }

  async function sendStop(current: Run): Promise<void> {
    if (current.sessionId === null) return;
    try {
      await dictationStop(current.sessionId);
    } catch {
      logFailure("dictation: stop failed");
      finish(current);
    }
  }

  /** Stops listening; what was heard is still transcribed and inserted. */
  async function stop(): Promise<void> {
    const current = run();
    if (!current || phase() === "stopping") return;
    setPhase("stopping");
    current.stopRequested = true;
    await sendStop(current);
  }

  /** Ends the session at once and drops what is not inserted yet. */
  function cancel(): void {
    const current = run();
    if (current) abandon(current);
  }

  async function toggle(): Promise<void> {
    if (run() === null) await start();
    else await stop();
  }

  const isLive = () => run() !== null;

  // A session writes into the tab it started in; another tab in that pane
  // ends it rather than writing into a different file.
  createEffect(() => {
    const current = run();
    if (!current) return;
    if (current.pane.currentBufferId() !== current.bufferId) {
      abandon(current);
      showToast(STOPPED_ON_TAB_CHANGE, "info");
    }
  });

  createEffect(() => {
    if (!isLive()) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      cancel();
    };
    const onPageHide = () => cancel();
    document.addEventListener("keydown", onKeyDown, true);
    window.addEventListener("pagehide", onPageHide);
    onCleanup(() => {
      document.removeEventListener("keydown", onKeyDown, true);
      window.removeEventListener("pagehide", onPageHide);
    });
  });

  createEffect(() => {
    if (startedAt() === null || phase() !== "recording") return;
    const timer = setInterval(() => setNow(Date.now()), 1000);
    onCleanup(() => clearInterval(timer));
  });

  /** Whole seconds the microphone has been on, or null before it is. */
  const elapsedSeconds = (): number | null => {
    const since = startedAt();
    return since === null ? null : Math.max(0, Math.floor((now() - since) / 1000));
  };

  return {
    phase,
    isLive,
    /** The microphone's level, 0 to 1. */
    level,
    /** The on-device model download, 0 to 1, or null when not known. */
    progress,
    /** Segments heard and not transcribed yet. */
    pending,
    elapsedSeconds,
    start,
    stop,
    cancel,
    toggle,
    /** Where "Open Dictation settings" on a failure goes. */
    attachSettingsOpener(open: (settingId: string) => void): void {
      openSettingsAt = open;
    },
  };
}

export type DictationStore = ReturnType<typeof createDictationStore>;
