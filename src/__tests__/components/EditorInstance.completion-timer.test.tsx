import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, cleanup } from "@solidjs/testing-library";
import { createSignal } from "solid-js";
import { EditorView } from "@codemirror/view";
import { completionStatus } from "@codemirror/autocomplete";
import type { BufferDocument } from "../../types/buffer";
import WindowProvider from "../../components/WindowProvider/WindowProvider";
import { WIKILINK_CLASS } from "../../editor/wikilink-decorations";

// Typing arms the completion plugin's debounce timer, and the plugin clears
// that timer nowhere. A file-type change that takes the completion out of the
// view before the timer fires used to throw "Field is not present in this
// state" from the timer callback.

const bufferContent = new Map<string, string>();

vi.mock("../../services/tauri", () => ({
  readBufferContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  saveBufferContent: vi.fn(async () => {}),
}));

vi.mock("../../services/events", () => ({
  onEvent: vi.fn(async () => () => {}),
  emitFrontendReady: vi.fn(async () => {}),
}));

vi.mock("../../stores/global/buffer-registry", () => ({
  bufferRegistry: {
    readContent: vi.fn(async (id: string) => bufferContent.get(id) ?? ""),
  },
}));

const MD_SOURCE = "# Plan\n\nSee [[Other]] here.\n";
const TXT_SOURCE = "milk\neggs\n";

function mockBuffer(id: string, name: string, sourcePath: string | null): BufferDocument {
  return {
    id,
    title: name,
    filename: name,
    status: "active",
    language: null,
    source_path: sourcePath,
    cursor_pos: 0,
    scroll_pos: 0,
    tab_order: 0,
    created_at: new Date().toISOString(),
    updated_at: new Date().toISOString(),
    closed_at: null,
    read_only: false,
    size_bytes: 0,
    line_ending: "lf",
  };
}

async function flushMicrotasks(n = 20) {
  for (let i = 0; i < n; i++) await Promise.resolve();
}

async function mount(buffer: BufferDocument) {
  const EditorInstance = (await import("../../components/Editor/EditorInstance")).default;
  const [buf, setBuf] = createSignal(buffer);
  const result = render(() => (
    <WindowProvider windowId={9502}>
      <EditorInstance buffer={buf()} />
    </WindowProvider>
  ));
  await flushMicrotasks();
  return { ...result, setBuf };
}

function viewIn(container: HTMLElement): EditorView {
  const dom = container.querySelector<HTMLElement>(".cm-editor");
  const view = dom ? EditorView.findFromDOM(dom) : null;
  if (!view) throw new Error("no editor view mounted");
  return view;
}

function typeAtEnd(view: EditorView, text: string) {
  const end = view.state.doc.length;
  view.dispatch({
    changes: { from: end, insert: text },
    selection: { anchor: end + text.length },
    userEvent: "input.type",
  });
}

describe("EditorInstance: a file-type change inside the completion debounce", () => {
  beforeEach(() => {
    bufferContent.clear();
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.useRealTimers();
    cleanup();
    vi.restoreAllMocks();
  });

  it("throws nothing when a .md buffer is renamed to .txt right after typing", async () => {
    bufferContent.set("R1", MD_SOURCE);
    const md = mockBuffer("R1", "plan.md", "/files/plan.md");
    const { container, setBuf } = await mount(md);
    const view = viewIn(container);

    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    typeAtEnd(view, "s");
    expect(completionStatus(view.state)).toBe("pending");

    setBuf({ ...md, title: "plan.txt", filename: "plan.txt", source_path: "/files/plan.txt" });
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).toBeNull();

    expect(() => vi.advanceTimersByTime(1_000)).not.toThrow();
  });

  it("throws nothing when a .txt tab is brought forward right after typing in a .md tab", async () => {
    bufferContent.set("S1", MD_SOURCE);
    bufferContent.set("S2", TXT_SOURCE);
    const { container, setBuf } = await mount(mockBuffer("S1", "plan.md", "/files/plan.md"));
    const view = viewIn(container);

    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    typeAtEnd(view, "s");
    expect(completionStatus(view.state)).toBe("pending");

    setBuf(mockBuffer("S2", "list.txt", "/files/list.txt"));
    // The outgoing view keeps its own file's type until it is replaced.
    expect(container.querySelector(`.${WIKILINK_CLASS}`)).not.toBeNull();

    expect(() => vi.advanceTimersByTime(1_000)).not.toThrow();
    await flushMicrotasks();
    expect(() => vi.advanceTimersByTime(1_000)).not.toThrow();
    expect(viewIn(container).state.doc.toString()).toBe(TXT_SOURCE);
  });
});
