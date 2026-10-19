// Built and driven by scripts/perf-editor.mjs; docs/perf-budgets.md says what it measures.
import { render } from "solid-js/web";
import { createEffect, createRoot } from "solid-js";
import "../../../src/styles/global.css";
import { getSearchQuery } from "@codemirror/search";
import { ChangeSet, Text } from "@codemirror/state";
import { undoDepth } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import App from "../../../src/App";
import { installIpc, type CommandArgs, type CommandHandler } from "../../../demo/ipc";
import { createBackend } from "../../../demo/backend/backend";
import { classifySize, formatHexDump, onceOpen, sniffsBinary } from "../../../demo/backend/classify";
import { windowRegistry } from "../../../src/stores/global/window-registry";
import { executeCommand } from "../../../src/commands/registry";
import { findStore } from "../../../src/stores/global/find-store";
import { largeFileOpen } from "../../../src/stores/global/large-file-open";
import { configStore } from "../../../src/stores/global/config";
import type { BufferDocument, FileOpenMode, FileOpenResult } from "../../../src/types/buffer";
import { generateCorpus, RARE_WORD, type CorpusSpec } from "../../../src/__tests__/editor/fixtures/editor-corpus";
import { WHOLE_DOCUMENT, filterTasksStartedIn, type BigCall, type LongTask } from "../budgets.mjs";

const HARNESS_ROOT = "/Users/you/Perf";

class HarnessError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "HarnessError";
  }
}

/** The host turning down a write, with the code Rust answers it with. */
class HostRefusal extends Error {
  constructor(code: string) {
    super(code);
    this.name = "HostRefusal";
  }
}

// While recording, every call that turns at least WHOLE_DOCUMENT.minChars
// characters into one value is noted, and every command the page sends is
// counted. The host's own work runs at depth, where nothing is noted.
const recording = { on: false, depth: 0, sending: null as string | null };
const bigCalls: BigCall[] = [];
const ipcCalls: Record<string, number> = {};

function hostSide<T>(work: () => T): T {
  recording.depth++;
  try {
    return work();
  } finally {
    recording.depth--;
  }
}

/** Whether a call taking `size` characters, known before it runs, is noted. */
const notes = (size: number) => recording.on && recording.depth === 0 && size >= WHOLE_DOCUMENT.minChars;

function noted<T>(kind: string, size: number, work: () => T): T {
  recording.depth++;
  const started = performance.now();
  try {
    return work();
  } finally {
    recording.depth--;
    bigCalls.push({ kind, size, ms: performance.now() - started });
  }
}

type Stringify = (value: unknown, replacer?: unknown, space?: string | number) => string | undefined;
const originalStringify = JSON.stringify as Stringify;
const recordingStringify: Stringify = (value, replacer, space) => {
  if (!recording.on || recording.depth > 0) return originalStringify(value, replacer, space);
  const kind = recording.sending ? `JSON.stringify (the IPC send of ${recording.sending})` : "JSON.stringify";
  recording.depth++;
  const started = performance.now();
  try {
    const out = originalStringify(value, replacer, space);
    if (typeof out === "string" && out.length >= WHOLE_DOCUMENT.minChars) {
      bigCalls.push({ kind, size: out.length, ms: performance.now() - started });
    }
    return out;
  } finally {
    recording.depth--;
  }
};
JSON.stringify = recordingStringify as typeof JSON.stringify;

const originalEncode = TextEncoder.prototype.encode;
TextEncoder.prototype.encode = function (this: TextEncoder, input?: string) {
  const size = input?.length ?? 0;
  if (!notes(size)) return originalEncode.call(this, input);
  return noted("TextEncoder.encode", size, () => originalEncode.call(this, input));
};
const originalEncodeInto = TextEncoder.prototype.encodeInto;
TextEncoder.prototype.encodeInto = function (this: TextEncoder, source: string, destination: Uint8Array) {
  if (!notes(source.length)) return originalEncodeInto.call(this, source, destination);
  return noted("TextEncoder.encodeInto", source.length, () => originalEncodeInto.call(this, source, destination));
};
const originalDecode = TextDecoder.prototype.decode;

// The document's two classes, a leaf of lines and a node over leaves.
for (const proto of new Set([Object.getPrototypeOf(Text.of(["a"])), Object.getPrototypeOf(Text.of(Array<string>(400).fill("a")))])) {
  const toString = proto.toString as (this: Text) => string;
  const sliceString = proto.sliceString as (this: Text, from: number, to?: number, lineSep?: string) => string;
  Object.defineProperty(proto, "toString", {
    configurable: true,
    writable: true,
    value: function (this: Text) {
      if (!notes(this.length)) return toString.call(this);
      return noted("Text.toString", this.length, () => toString.call(this));
    },
  });
  Object.defineProperty(proto, "sliceString", {
    configurable: true,
    writable: true,
    value: function (this: Text, from: number, to?: number, lineSep?: string) {
      const size = (to ?? this.length) - from;
      if (!notes(size)) return sliceString.call(this, from, to, lineSep);
      return noted("Text.sliceString", size, () => sliceString.call(this, from, to, lineSep));
    },
  });
}

function startRecording(): void {
  bigCalls.length = 0;
  for (const key of Object.keys(ipcCalls)) delete ipcCalls[key];
  recording.on = true;
}

function stopRecording() {
  recording.on = false;
  return { bigCalls: bigCalls.slice(), ipcCalls: { ...ipcCalls } };
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
  recording.sending = cmd;
  let body: string;
  try {
    body = JSON.stringify(args ?? {}, tauriReplacer);
  } finally {
    recording.sending = null;
  }
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
    encoding: "UTF-8",
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

// With keepWrites on, a save lands in the file's bytes and moves its digest,
// so a later read gives back what was saved, as the Rust side does.
const writes = { kept: false, count: 0 };

function keepWrite(file: HarnessFile, cmd: string, args: Record<string, unknown>): void {
  let text: string;
  if (cmd === "save_buffer_changes") {
    if (args.base !== file.digest) throw new HostRefusal("ERR_CHANGES_STALE");
    const current = originalDecode.call(new TextDecoder(), bytesOf(file));
    text = ChangeSet.fromJSON(args.changes).apply(Text.of(current.split("\n"))).toString();
  } else {
    text = String(args.content ?? "");
  }
  file.bytes = originalEncode.call(new TextEncoder(), text);
  file.size = file.bytes.length;
  writes.count++;
  file.digest = `${file.digest.slice(0, 64)}-w${writes.count}`;
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
    // Every write to the page's files is the editor's own, so each file is as
    // Writ last read or wrote it.
    case "note_file_unchanged":
      return { value: true };
    case "get_buffer":
      return { value: file.doc ? { ...file.doc } : null };
    // Unless keepWrites is on, the write is not kept: the page measures the
    // frontend's side of a save, and the digest stays the one the open recorded.
    case "save_buffer_content":
    case "save_buffer_content_unindexed":
    case "save_buffer_changes":
      if (writes.kept) keepWrite(file, cmd, args);
      return { value: file.digest };
    case "set_buffer_view_position":
      return { value: null };
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
  return (cmd, args, raw) => {
    if (recording.on) ipcCalls[cmd] = (ipcCalls[cmd] ?? 0) + 1;
    // A raw body goes to the host as it is; only JSON arguments are serialised.
    if (!raw) chargeSend(cmd, args);
    if (cmd === "report_first_paint") {
      firstPaints.push({ mode: String(args?.mode), elapsedMs: Number(args?.elapsedMs) });
      return null;
    }
    return hostSide(() => {
      const own = answerFor(cmd, args ?? {});
      if (own) return own.value;
      return backend.handle(cmd, args);
    });
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
  const corpus = hostSide(() => generateCorpus(spec));
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
  startRecording();
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
    return {
      id: doc.id,
      firstPaintMs: painted - (confirmedAt ?? started),
      confirmed: confirmedAt !== null,
      ...observations(),
      ...stopRecording(),
    };
  } finally {
    recording.on = false;
    stopAnswering();
  }
}

/**
 * Runs Split right on the file in front and resolves once the second pane's
 * editor holds it, answering what each pane's editor holds.
 */
async function splitRight() {
  const win = activeWindow();
  const id = win.tabs.activeTabId();
  if (!id) throw new HarnessError("no file in front to split");
  if (!executeCommand("pane.splitRight")) throw new HarnessError("Split right did not run");
  const right = win.editor.forPane("right");
  const started = performance.now();
  while (right.currentBufferId() !== id) {
    if (performance.now() - started > 20_000) throw new HarnessError("the second pane never showed the file");
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  await nextFrame();
  const left = win.editor.forPane("left").getView();
  return {
    focused: win.panes.focused(),
    leftLength: left?.state.doc.length ?? 0,
    rightLength: right.getView()?.state.doc.length ?? 0,
  };
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

// What the Inline block widgets have drawn on screen, so a scenario of tables
// and diagrams can tell that it measured them and not their source.
function blocks() {
  const dom = activeWindow().editor.getView()?.dom;
  const count = (selector: string) => dom?.querySelectorAll(selector).length ?? 0;
  return {
    tables: count(".cm-md-table-widget"),
    diagrams: count(".cm-md-diagram[data-status='ready'] svg"),
    failedDiagrams: count(".cm-md-diagram[data-status='failed']"),
    math: count(".cm-md-math-block[data-status='ready'] .katex, .cm-md-math-inline[data-status='ready'] .katex"),
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

/** Shows the word, character and token counts in the status bar, or hides them. */
async function setCounts(on: boolean): Promise<void> {
  const config = configStore.config();
  await configStore.save({ ...config, editor: { ...config.editor, status_bar_counts: on } });
}

/** The counts the status bar shows, once they are exact; null on timeout. */
async function waitCountsExact(timeoutMs: number) {
  const started = performance.now();
  while (performance.now() - started < timeoutMs) {
    const counts = activeWindow().counts.counts();
    if (counts?.exact) {
      return { ...counts, settledAt: performance.now(), shown: document.querySelector(".statusbar-field--words")?.textContent ?? "" };
    }
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
  return null;
}

/** The counts of the document in the view, the way the status bar counted them before it counted the document's strings. */
function wholeTextCounts() {
  const text = hostSide(() => activeWindow().editor.getView()?.state.doc.toString() ?? "");
  const trimmed = text.trim();
  return { words: trimmed === "" ? 0 : trimmed.split(/\s+/).length, characters: [...text].length };
}

function emitWindowShown(): void {
  emitEvent?.("writ://window-shown", { kind: "window:shown", payload: { rust_elapsed_us: 0 } });
}

/** The undo depth, the selection and the line at the top of the view of the editor in front. */
function editorSnapshot() {
  const view = activeWindow().editor.getView();
  if (!view) return null;
  const visibleTop = view.scrollDOM.getBoundingClientRect().top - view.documentTop;
  return {
    bufferId: activeWindow().editor.currentBufferId(),
    undoDepth: undoDepth(view.state),
    anchor: view.state.selection.main.anchor,
    head: view.state.selection.main.head,
    topLine: view.state.doc.lineAt(view.lineBlockAtHeight(Math.max(0, visibleTop)).from).number,
    length: view.state.doc.length,
  };
}

/** Selects a few characters of the line at `fraction` of the document, scrolled to the middle of the view. */
async function placeSelection(fraction: number) {
  const view = activeWindow().editor.getView();
  if (!view) throw new HarnessError("no editor to place the selection in");
  const line = view.state.doc.lineAt(Math.floor(view.state.doc.length * fraction));
  view.dispatch({
    selection: { anchor: line.from + Math.min(3, line.length), head: line.from + Math.min(12, line.length) },
    effects: EditorView.scrollIntoView(line.from, { y: "center" }),
  });
  await nextFrame();
  await nextFrame();
  return editorSnapshot();
}

async function waitUntil(test: () => boolean, what: string, timeoutMs = 30_000): Promise<void> {
  const started = performance.now();
  while (!test()) {
    if (performance.now() - started > timeoutMs) throw new HarnessError(`${what} timed out`);
    await new Promise((resolve) => requestAnimationFrame(resolve));
  }
}

/** Switches from the tab in front to `otherId` and back, answering what the editor kept and what the switch back sent. */
async function tabRoundTrip(otherId: string) {
  const win = activeWindow();
  const id = win.editor.currentBufferId();
  if (!id) throw new HarnessError("no file in front to switch away from");
  const before = editorSnapshot();
  win.tabs.setActiveTabId(otherId);
  await waitUntil(() => win.editor.currentBufferId() === otherId, "the switch away");
  await nextFrame();
  await new Promise((resolve) => setTimeout(resolve, 1500));
  startRecording();
  try {
    const started = performance.now();
    win.tabs.setActiveTabId(id);
    await waitUntil(() => win.editor.currentBufferId() === id, "the switch back");
    const switchBackMs = (await nextFrame()) - started;
    await nextFrame();
    return { before, after: editorSnapshot(), switchBackMs, ...stopRecording() };
  } finally {
    recording.on = false;
  }
}

function keepWrites(on: boolean): void {
  writes.kept = on;
}

function isReady(): boolean {
  return windowRegistry.getActive()?.editor.getView() != null;
}

Object.assign(window, {
  __harness: {
    addFile,
    dropBytes,
    open,
    splitRight,
    features,
    blocks,
    focusEditor,
    startKeys,
    stopKeys,
    waitViewportAtEnd,
    findStatus,
    waitFindSettled,
    setCounts,
    waitCountsExact,
    wholeTextCounts,
    resetObservations,
    observations,
    startRecording,
    stopRecording,
    editorSnapshot,
    placeSelection,
    tabRoundTrip,
    keepWrites,
    firstPaints: () => firstPaints.slice(),
    emitWindowShown,
    ready: isReady,
  },
});

const root = document.getElementById("app");
if (!root) throw new HarnessError("the page has no #app element");
render(() => <App />, root);
