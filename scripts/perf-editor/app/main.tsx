// Built and driven by scripts/perf-editor.mjs; docs/perf-budgets.md says what it measures.
import { render } from "solid-js/web";
import { createEffect, createRoot } from "solid-js";
import "../../../src/styles/global.css";
import { getSearchQuery } from "@codemirror/search";
import App from "../../../src/App";
import { installIpc, type CommandArgs, type CommandHandler } from "../../../demo/ipc";
import { createBackend } from "../../../demo/backend/backend";
import { classifySize, formatHexDump, onceOpen, sniffsBinary } from "../../../demo/backend/classify";
import { windowRegistry } from "../../../src/stores/global/window-registry";
import { findStore } from "../../../src/stores/global/find-store";
import { largeFileOpen } from "../../../src/stores/global/large-file-open";
import type { BufferDocument, FileOpenMode, FileOpenResult } from "../../../src/types/buffer";
import { generateCorpus, RARE_WORD, type CorpusSpec } from "../../../src/__tests__/editor/fixtures/editor-corpus";
import { filterTasksStartedIn, type LongTask } from "../budgets.mjs";

const HARNESS_ROOT = "/Users/you/Perf";

class HarnessError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "HarnessError";
  }
}

interface SendCost {
  calls: number;
  chars: number;
  ms: number;
}
const sendCosts: Record<string, SendCost> = {};

// Tauri's process-ipc-message-fn.js serialises every call's arguments with
// JSON.stringify and this replacer before the host sees them.
function tauriReplacer(_key: string, value: unknown): unknown {
  if (value instanceof Map) return Object.fromEntries(value.entries());
  if (value instanceof Uint8Array) return Array.from(value);
  if (value instanceof ArrayBuffer) return Array.from(new Uint8Array(value));
  return value;
}

function chargeSend(cmd: string, args: CommandArgs): void {
  const started = performance.now();
  const body = JSON.stringify(args ?? {}, tauriReplacer);
  const cost = (sendCosts[cmd] ??= { calls: 0, chars: 0, ms: 0 });
  cost.calls++;
  cost.chars += body.length;
  cost.ms += performance.now() - started;
}

interface HarnessFile {
  path: string;
  size: number;
  bytes: Uint8Array | null;
  digest: string;
  doc: BufferDocument | null;
}

const files = new Map<string, HarnessFile>();
const filesById = new Map<string, HarnessFile>();
let nextId = 1;

function rowFor(file: HarnessFile, mode: FileOpenMode): BufferDocument {
  if (file.doc) return file.doc;
  const now = new Date().toISOString();
  const id = `perf-${nextId++}`;
  // writ_core::buffer::manager::open_external: titled with the file name.
  const doc: BufferDocument = {
    id,
    title: file.path.slice(file.path.lastIndexOf("/") + 1),
    filename: `${id}.txt`,
    status: "active",
    language: null,
    source_path: file.path,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 99,
    created_at: now,
    updated_at: now,
    closed_at: null,
    read_only: mode.kind === "Binary",
    size_bytes: file.size,
    line_ending: "lf",
  };
  file.doc = doc;
  filesById.set(id, file);
  return doc;
}

function bytesOf(file: HarnessFile): Uint8Array {
  if (!file.bytes) throw new HarnessError(`${file.path} was read after its bytes were dropped`);
  return file.bytes;
}

function openAnswer(file: HarnessFile, confirmed: boolean): FileOpenResult {
  const mode = classifySize(file.size, sniffsBinary(bytesOf(file)));
  if (mode.kind === "Refused") return { doc: null, mode, size_bytes: file.size };
  if (mode.kind === "LargeFileConfirm" && !confirmed && !file.doc) return { doc: null, mode, size_bytes: file.size };
  const opened = onceOpen(mode);
  return { doc: { ...rowFor(file, opened) }, mode: opened, size_bytes: file.size };
}

function answerFor(cmd: string, args: Record<string, unknown>): { value: unknown } | null {
  const path = typeof args.path === "string" ? args.path : null;
  const id = typeof args.id === "string" ? args.id : null;
  const file = (path && files.get(path)) || (id && filesById.get(id)) || null;
  if (!file) return null;
  switch (cmd) {
    case "open_file":
      return { value: openAnswer(file, false) };
    case "open_file_confirmed":
      return { value: openAnswer(file, true) };
    case "read_buffer_content": {
      const bytes = bytesOf(file);
      if (file.doc?.read_only) return { value: new TextEncoder().encode(formatHexDump(bytes, file.size)).buffer };
      return { value: bytes.slice().buffer };
    }
    case "note_disk_state":
      return { value: { state: "described", disk: { hash: file.digest, size: file.size, mtime_ms: Date.now() } } };
    case "get_buffer":
      return { value: file.doc ? { ...file.doc } : null };
    // The write is not kept: the page measures the frontend's side of a save,
    // and the digest stays the one the open recorded.
    case "save_buffer_content":
    case "save_buffer_content_unindexed":
      return { value: file.digest };
    case "auto_retitle_note":
      return { value: { kind: "skipped" } };
    case "close_buffer":
    case "close_buffers":
    case "update_tab_order":
    case "rename_buffer":
      return { value: null };
    default:
      return null;
  }
}

const firstPaints: { mode: string; elapsedMs: number }[] = [];
let emitEvent: ((event: string, payload: unknown) => void) | null = null;

installIpc((bridge) => {
  emitEvent = (event, payload) => bridge.emit(event, payload);
  const backend = createBackend(bridge);
  return (cmd, args) => {
    chargeSend(cmd, args);
    if (cmd === "report_first_paint") {
      firstPaints.push({ mode: String(args?.mode), elapsedMs: Number(args?.elapsedMs) });
      return null;
    }
    const own = answerFor(cmd, args ?? {});
    if (own) return own.value;
    return backend.handle(cmd, args);
  };
});

// The preview pane keeps an iframe on the host's writ-preview:// scheme. A
// browser has no handler for it; the blank page is what the host serves.
const hostOnly = (value: string) => (value.startsWith("writ-preview://") ? "about:blank" : value);
const frameSrc = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "src");
if (frameSrc?.set) {
  const setSrc = frameSrc.set;
  Object.defineProperty(HTMLIFrameElement.prototype, "src", {
    ...frameSrc,
    set(this: HTMLIFrameElement, value: string) {
      setSrc.call(this, hostOnly(value));
    },
  });
}
const setAttribute = Element.prototype.setAttribute;
HTMLIFrameElement.prototype.setAttribute = function (name: string, value: string) {
  setAttribute.call(this, name, name === "src" ? hostOnly(value) : value);
};

const longTasks: LongTask[] = [];
let windowStart = 0;

new PerformanceObserver((list) => {
  for (const entry of list.getEntries()) longTasks.push({ start: entry.startTime, duration: entry.duration });
}).observe({ type: "longtask", buffered: true });

// Keydown to the end of the next frame, per key, while recording.
interface KeySample {
  key: string;
  at: number;
  ms: number;
}
const keySamples: KeySample[] = [];
let recordingKeys = false;
window.addEventListener(
  "keydown",
  (event) => {
    if (!recordingKeys) return;
    const at = event.timeStamp;
    const key = event.key;
    requestAnimationFrame(() => setTimeout(() => keySamples.push({ key, at, ms: performance.now() - at }), 0));
  },
  { capture: true },
);

function activeWindow() {
  const active = windowRegistry.getActive();
  if (!active) throw new HarnessError("no active window");
  return active;
}

const nextFrame = () =>
  new Promise<number>((resolve) => requestAnimationFrame(() => setTimeout(() => resolve(performance.now()), 0)));

/** Occurrences of an ASCII word in `bytes`, ignoring ASCII case: what Find counts for it in a generated file. */
function countWord(bytes: Uint8Array, word: string): number {
  const needle = [...word.toLowerCase()].map((ch) => ch.charCodeAt(0));
  const last = bytes.length - needle.length;
  let count = 0;
  for (let at = 0; at <= last; at++) {
    let i = 0;
    while (i < needle.length && (bytes[at + i] | 0x20) === needle[i]) i++;
    if (i === needle.length) {
      count++;
      at += needle.length - 1;
    }
  }
  return count;
}

async function addFile(spec: CorpusSpec) {
  const corpus = generateCorpus(spec);
  const digestBuffer = await crypto.subtle.digest("SHA-256", corpus.bytes as Uint8Array<ArrayBuffer>);
  const digest = [...new Uint8Array(digestBuffer)].map((byte) => byte.toString(16).padStart(2, "0")).join("");
  const path = `${HARNESS_ROOT}/${spec.name}`;
  files.set(path, { path, size: corpus.bytes.length, bytes: corpus.bytes, digest, doc: null });
  return { path, size: corpus.bytes.length, lines: corpus.lines, rareWordCount: countWord(corpus.bytes, RARE_WORD) };
}

/** Lets the generated files go, so the heap holds only what the editor keeps. */
function dropBytes(): void {
  for (const file of files.values()) file.bytes = null;
}

function resetObservations(): void {
  longTasks.length = 0;
  windowStart = performance.now();
  for (const key of Object.keys(sendCosts)) delete sendCosts[key];
}

function observations() {
  return {
    longTasks: filterTasksStartedIn(longTasks, windowStart, performance.now()),
    sendCosts: JSON.parse(JSON.stringify(sendCosts)) as Record<string, SendCost>,
  };
}

/** Opens `path` and resolves once its first frame is painted, answering Open when the app asks. */
async function open(path: string) {
  resetObservations();
  const win = activeWindow();
  let confirmedAt: number | null = null;
  const stopAnswering = createRoot((dispose) => {
    createEffect(() => {
      if (largeFileOpen.state()?.phase !== "asking") return;
      confirmedAt = performance.now();
      queueMicrotask(() => largeFileOpen.respond(true));
    });
    return dispose;
  });
  try {
    const started = performance.now();
    const doc = await win.tabs.openFile(path);
    if (!doc) throw new HarnessError(`opening ${path} gave no tab`);
    while (win.editor.currentBufferId() !== doc.id) await new Promise((resolve) => requestAnimationFrame(resolve));
    const painted = await nextFrame();
    return { firstPaintMs: painted - (confirmedAt ?? started), confirmed: confirmedAt !== null, ...observations() };
  } finally {
    stopAnswering();
  }
}

function features() {
  const win = activeWindow();
  const view = win.editor.getView();
  return {
    tier: win.editor.largeFileMode()?.kind ?? "Normal",
    lineWrapping: view?.lineWrapping ?? null,
    docLength: view?.state.doc.length ?? 0,
    docLines: view?.state.doc.lines ?? 0,
  };
}

function focusEditor(): void {
  activeWindow().editor.getView()?.focus();
}

function startKeys(): void {
  keySamples.length = 0;
  recordingKeys = true;
}

function stopKeys(): KeySample[] {
  recordingKeys = false;
  return keySamples.slice();
}

async function waitViewportAtEnd(timeoutMs: number): Promise<number | null> {
  const view = activeWindow().editor.getView();
  if (!view) return null;
  const started = performance.now();
  while (performance.now() - started < timeoutMs) {
    const end = view.state.doc.length;
    if (view.viewport.to >= end && view.state.selection.main.head >= end) return performance.now();
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  return null;
}

function findStatus() {
  const view = activeWindow().editor.getView();
  return {
    typed: findStore.queryText(),
    applied: view ? getSearchQuery(view.state).search : null,
    matches: findStore.matches(),
  };
}

/** The time of the first frame whose overlay holds the final count for the typed query; null on timeout. */
async function waitFindSettled(timeoutMs: number): Promise<number | null> {
  const started = performance.now();
  while (performance.now() - started < timeoutMs) {
    const status = findStatus();
    if (status.typed === status.applied && !status.matches.pending) return performance.now();
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  return null;
}

function emitWindowShown(): void {
  emitEvent?.("writ://window-shown", { kind: "window:shown", payload: { rust_elapsed_us: 0 } });
}

function isReady(): boolean {
  return windowRegistry.getActive()?.editor.getView() != null;
}

Object.assign(window, {
  __harness: {
    addFile,
    dropBytes,
    open,
    features,
    focusEditor,
    startKeys,
    stopKeys,
    waitViewportAtEnd,
    findStatus,
    waitFindSettled,
    resetObservations,
    observations,
    firstPaints: () => firstPaints.slice(),
    emitWindowShown,
    ready: isReady,
  },
});

const root = document.getElementById("app");
if (!root) throw new HarnessError("the page has no #app element");
render(() => <App />, root);
