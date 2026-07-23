import { createSignal, createRoot, createEffect } from "solid-js";
import type { ChangeSet } from "@codemirror/state";
import { windowRegistry } from "./window-registry";
import { showToast } from "../../components/Notifications/Toast";
import {
  aiRewrite,
  aiCancel,
  aiHasApiKey,
  aiSetApiKey,
  aiClearApiKey,
  type AiAction,
  type AiKeyState,
} from "../../services/tauri";
import type { WritEvent } from "../../types/events";

export type { AiKeyState };

type AiRewritePayload = Extract<WritEvent, { kind: "ai:rewrite" }>["payload"];

export type AiStatus = "idle" | "awaiting-instruction" | "streaming" | "done" | "error";

/** Where a rewrite writes back, anchored when the request starts. */
export interface AnchoredRange {
  from: number;
  to: number;
  text: string;
  usedSelection: boolean;
  bufferId: string;
}

interface Session {
  requestId: string;
  bufferId: string;
  from: number;
  to: number;
  original: string;
  action: AiAction;
}

const ACTION_LABELS: Record<AiAction, string> = {
  proofread: "Proofread",
  rephrase: "Rephrase",
  polish: "Polish",
  custom: "Custom rewrite",
};

// Singleton state — Writ is single-window. The overlay and its stream are a
// single live session at a time.
function createAiRewriteStore() {
  const [session, setSession] = createSignal<Session | null>(null);
  const [result, setResult] = createSignal("");
  const [status, setStatus] = createSignal<AiStatus>("idle");
  const [errorMessage, setErrorMessage] = createSignal("");
  const [instruction, setInstruction] = createSignal("");

  // Switching or closing the anchored tab discards the preview: the offsets no
  // longer refer to the buffer the user is looking at.
  createEffect(() => {
    const s = session();
    if (!s) return;
    const activeId = windowRegistry.getActive()?.editor.currentBufferId();
    if (activeId != null && activeId !== s.bufferId) {
      abort("Rewrite discarded: the tab changed.");
    }
  });

  function reset() {
    setSession(null);
    setResult("");
    setStatus("idle");
    setErrorMessage("");
    setInstruction("");
  }

  function launch(action: AiAction, customInstruction?: string) {
    const s = session();
    if (!s) return;
    setResult("");
    setErrorMessage("");
    setStatus("streaming");
    // The request id is already on the session (set synchronously in start), so
    // events cannot arrive before we can match them — an immediate error, such
    // as a refused localhost connection, is never dropped.
    aiRewrite(s.requestId, action, s.original, customInstruction).catch((err) => {
      if (session() === s) {
        setStatus("error");
        setErrorMessage(readableError(err));
      }
    });
  }

  /** Begins a rewrite over `range`. `custom` opens for an instruction first. */
  function start(action: AiAction, range: AnchoredRange) {
    reset();
    setSession({
      requestId: newRequestId(),
      bufferId: range.bufferId,
      from: range.from,
      to: range.to,
      original: range.text,
      action,
    });
    if (action === "custom") {
      setStatus("awaiting-instruction");
      return;
    }
    launch(action);
  }

  function submitInstruction() {
    const text = instruction().trim();
    if (!text || status() !== "awaiting-instruction") return;
    launch("custom", text);
  }

  function handleStreamEvent(payload: AiRewritePayload) {
    const s = session();
    if (!s || !s.requestId || payload.request_id !== s.requestId) return;
    if (payload.kind === "chunk") {
      if (status() === "streaming") setResult((r) => r + (payload.text ?? ""));
    } else if (payload.kind === "done") {
      if (status() === "streaming") setStatus("done");
    } else if (payload.kind === "error") {
      setStatus("error");
      setErrorMessage(payload.text ?? "Rewrite failed.");
    }
  }

  // Anchored-range bookkeeping: an edit that touches the range invalidates the
  // preview; edits elsewhere shift the anchor so it keeps pointing at the same
  // text. Called for every document change in the active buffer.
  function onDocChanged(bufferId: string, changes: ChangeSet) {
    const s = session();
    if (!s || bufferId !== s.bufferId) return;
    if (changes.touchesRange(s.from, s.to)) {
      abort("Rewrite discarded: the text was edited.");
      return;
    }
    setSession({ ...s, from: changes.mapPos(s.from, 1), to: changes.mapPos(s.to, -1) });
  }

  /** Writes the result over the anchored range in one undo step. */
  function apply() {
    const s = session();
    if (!s || status() !== "done") return;
    const text = result();
    const editor = windowRegistry.getActive()?.editor;
    // Clear first so the replacement dispatch is not seen as an aborting edit.
    reset();
    editor?.replaceRange(s.from, s.to, text);
  }

  /** Dismisses the overlay, cancelling an in-flight stream. */
  function discard() {
    cancelIfStreaming();
    reset();
    windowRegistry.getActive()?.editor.focusEditor();
  }

  function abort(reason: string) {
    if (!session()) return;
    cancelIfStreaming();
    reset();
    showToast(reason, "info");
  }

  function cancelIfStreaming() {
    const s = session();
    if (s?.requestId && status() === "streaming") void aiCancel(s.requestId);
  }

  return {
    isOpen: () => session() !== null,
    status,
    result,
    errorMessage,
    instruction,
    setInstruction,
    original: () => session()?.original ?? "",
    actionLabel: () => {
      const s = session();
      return s ? ACTION_LABELS[s.action] : "";
    },
    start,
    submitInstruction,
    handleStreamEvent,
    onDocChanged,
    apply,
    discard,
    hasApiKey: (preset: string): Promise<AiKeyState> => aiHasApiKey(preset),
    setApiKey: (preset: string, key: string): Promise<AiKeyState> => aiSetApiKey(preset, key),
    clearApiKey: (preset: string): Promise<AiKeyState> => aiClearApiKey(preset),
  };
}

function newRequestId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `air-${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

function readableError(err: unknown): string {
  if (typeof err === "string") return err;
  if (err instanceof Error) return err.message;
  return "Rewrite failed.";
}

export type AiRewriteStore = ReturnType<typeof createAiRewriteStore>;
export const aiRewriteStore = createRoot(createAiRewriteStore);
